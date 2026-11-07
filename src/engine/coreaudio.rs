//! CoreAudio plumbing: device enumeration, process tap, private aggregate
//! device and the real-time IO callback.

use super::Shared;
use crate::dsp::processor::{EqSettings, Processor};
use anyhow::{Result, anyhow, bail};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, Message};
use objc2_core_audio::*;
use objc2_core_audio_types::{AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp};
use objc2_core_foundation::CFDictionary;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
use std::ffi::{CStr, c_void};
use std::mem::{MaybeUninit, size_of};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::{self, NonNull};
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Instant;

const AGG_UID_PREFIX: &str = "dev.flatline.aggregate";
const SCRATCH_FRAMES: usize = 4096;
pub const SCOPE_CAPACITY: usize = 1 << 15;

// ------------------------------------------------------------ helpers ----

fn status_str(st: i32) -> String {
    let b = st.to_be_bytes();
    if b.iter().all(|c| c.is_ascii_graphic() || *c == b' ') {
        format!("'{}' ({st})", String::from_utf8_lossy(&b))
    } else {
        st.to_string()
    }
}

fn check(st: i32, what: &str) -> Result<()> {
    if st == 0 { Ok(()) } else { Err(anyhow!("{what} failed: {}", status_str(st))) }
}

fn addr(sel: u32, scope: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress { mSelector: sel, mScope: scope, mElement: kAudioObjectPropertyElementMain }
}

const SYS: AudioObjectID = kAudioObjectSystemObject as AudioObjectID;
const GLOBAL: u32 = kAudioObjectPropertyScopeGlobal;
const OUTPUT: u32 = kAudioObjectPropertyScopeOutput;
const INPUT: u32 = kAudioObjectPropertyScopeInput;

fn get_q<T: Copy, Q>(obj: AudioObjectID, sel: u32, scope: u32, qual: Option<&Q>) -> Result<T> {
    let a = addr(sel, scope);
    let mut v = MaybeUninit::<T>::zeroed();
    let mut size = size_of::<T>() as u32;
    let (qs, qp) = match qual {
        Some(q) => (size_of::<Q>() as u32, q as *const Q as *const c_void),
        None => (0, ptr::null()),
    };
    let st = unsafe {
        AudioObjectGetPropertyData(
            obj,
            NonNull::from(&a),
            qs,
            qp,
            NonNull::from(&mut size),
            NonNull::new_unchecked(v.as_mut_ptr() as *mut c_void),
        )
    };
    check(st, "AudioObjectGetPropertyData")?;
    Ok(unsafe { v.assume_init() })
}

fn get<T: Copy>(obj: AudioObjectID, sel: u32, scope: u32) -> Result<T> {
    get_q::<T, ()>(obj, sel, scope, None)
}

fn set<T: Copy>(obj: AudioObjectID, sel: u32, scope: u32, value: T) -> Result<()> {
    let a = addr(sel, scope);
    let st = unsafe {
        AudioObjectSetPropertyData(
            obj,
            NonNull::from(&a),
            0,
            ptr::null(),
            size_of::<T>() as u32,
            NonNull::from(&value).cast(),
        )
    };
    check(st, "AudioObjectSetPropertyData")
}

/// Variable-size property, returned as 8-byte aligned storage.
fn get_bytes(obj: AudioObjectID, sel: u32, scope: u32) -> Result<(Vec<u64>, usize)> {
    let a = addr(sel, scope);
    let mut size = 0u32;
    check(
        unsafe { AudioObjectGetPropertyDataSize(obj, NonNull::from(&a), 0, ptr::null(), NonNull::from(&mut size)) },
        "AudioObjectGetPropertyDataSize",
    )?;
    let mut buf = vec![0u64; (size as usize).div_ceil(8).max(1)];
    check(
        unsafe {
            AudioObjectGetPropertyData(
                obj,
                NonNull::from(&a),
                0,
                ptr::null(),
                NonNull::from(&mut size),
                NonNull::new_unchecked(buf.as_mut_ptr() as *mut c_void),
            )
        },
        "AudioObjectGetPropertyData",
    )?;
    Ok((buf, size as usize))
}

fn get_ids(obj: AudioObjectID, sel: u32, scope: u32) -> Result<Vec<AudioObjectID>> {
    let (buf, size) = get_bytes(obj, sel, scope)?;
    let p = buf.as_ptr() as *const AudioObjectID;
    Ok((0..size / 4).map(|i| unsafe { *p.add(i) }).collect())
}

fn get_string(obj: AudioObjectID, sel: u32, scope: u32) -> Result<String> {
    let raw: *mut NSString = get::<*mut NSString>(obj, sel, scope)?;
    // The HAL returns a +1 CFStringRef (toll-free bridged with NSString).
    let s: Retained<NSString> = unsafe { Retained::from_raw(raw) }.ok_or_else(|| anyhow!("null string"))?;
    Ok(s.to_string())
}

/// Channel count of each stream buffer of a device in a scope.
fn stream_layout(dev: AudioObjectID, scope: u32) -> Vec<u32> {
    let Ok((buf, size)) = get_bytes(dev, kAudioDevicePropertyStreamConfiguration, scope) else {
        return Vec::new();
    };
    if size < 4 {
        return Vec::new();
    }
    let abl = buf.as_ptr() as *const AudioBufferList;
    unsafe {
        let n = (*abl).mNumberBuffers as usize;
        let bufs = std::slice::from_raw_parts((*abl).mBuffers.as_ptr(), n);
        bufs.iter().map(|b| b.mNumberChannels).collect()
    }
}

fn as_obj<T: Message>(r: &T) -> &AnyObject {
    // Every Objective-C object is an AnyObject.
    unsafe { &*(r as *const T as *const AnyObject) }
}

fn nsstr(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

fn key(k: &CStr) -> Retained<NSString> {
    NSString::from_str(k.to_str().expect("ascii key"))
}

/// Let the HAL deliver its notifications on its own thread instead of the
/// main run loop (otherwise aggregate/tap setup can stall when the main
/// thread is not running a CFRunLoop, e.g. in --headless mode).
fn detach_hal_run_loop() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = set::<*const c_void>(SYS, kAudioHardwarePropertyRunLoop, GLOBAL, ptr::null());
    });
}

// ------------------------------------------------------------ devices ----

#[derive(Clone, Debug, PartialEq)]
/// An audio output device.
pub struct Device {
    /// CoreAudio object id (valid while the device is connected).
    pub id: AudioObjectID,
    /// Persistent unique id, stable across reboots and reconnections.
    pub uid: String,
    /// Human-readable name.
    pub name: String,
}

/// Lists every device that has output channels (Flatline's own private
/// aggregate devices excluded).
pub fn list_output_devices() -> Vec<Device> {
    let Ok(ids) = get_ids(SYS, kAudioHardwarePropertyDevices, GLOBAL) else {
        return Vec::new();
    };
    ids.into_iter()
        .filter(|&id| stream_layout(id, OUTPUT).iter().sum::<u32>() > 0)
        .filter_map(|id| {
            let uid = get_string(id, kAudioDevicePropertyDeviceUID, GLOBAL).ok()?;
            if uid.starts_with(AGG_UID_PREFIX) {
                return None;
            }
            let name = get_string(id, kAudioObjectPropertyName, GLOBAL).unwrap_or_else(|_| uid.clone());
            Some(Device { id, uid, name })
        })
        .collect()
}

/// UID of the current system default output device.
pub fn default_output_uid() -> Option<String> {
    let id: AudioObjectID = get(SYS, kAudioHardwarePropertyDefaultOutputDevice, GLOBAL).ok()?;
    get_string(id, kAudioDevicePropertyDeviceUID, GLOBAL).ok()
}

// ------------------------------------------------------------- engine ----

#[derive(Clone, Debug)]
#[allow(dead_code)] // diagnostics fields are printed by --headless
pub struct EngineInfo {
    pub device: Device,
    pub sample_rate: f64,
    pub buffer_frames: u32,
    pub tap_channels: u32,
    /// Output channels used (1-based, device numbering).
    pub out_channels: (u32, u32),
    /// Device output latency + safety offset, in frames.
    pub device_latency_frames: u32,
    pub in_layout: Vec<u32>,
    pub out_layout: Vec<u32>,
    pub tap_buf: usize,
    /// "device tap" (low latency) or "global mix" (fallback).
    pub tap_mode: &'static str,
    /// (device latency, safety offset) of the aggregate, input scope; per-stream input latencies.
    pub agg_input_latency: (u32, u32, Vec<u32>),
}

/// A running system-wide correction.
///
/// Dropping it stops the IO callback and destroys the tap and the aggregate
/// device: system audio goes straight to the device again.
pub struct Engine {
    agg: AudioObjectID,
    tap: AudioObjectID,
    proc_id: AudioDeviceIOProcID,
    state: *mut IoState,
    params: rtrb::Producer<EqSettings>,
    /// Pre/post mono samples streamed from the audio thread to the analyzer.
    pub scope: rtrb::Consumer<[f32; 2]>,
    /// What the engine is running on.
    pub info: EngineInfo,
}

struct IoState {
    proc: Processor,
    params: rtrb::Consumer<EqSettings>,
    scope: rtrb::Producer<[f32; 2]>,
    shared: Arc<Shared>,
    tap_buf: usize,
    /// Channels of the tap stream that feed L / R.
    tap_ch: [usize; 2],
    /// Copy every tap channel to output stream 0 first (multi-channel
    /// interfaces: channels we don't correct still play).
    passthrough: bool,
    out_l: (usize, usize),
    out_r: (usize, usize),
    scratch: Box<[[f32; 2]]>,
    dry: Box<[f32]>,
    sample_period: f64,
}

impl Engine {
    /// Starts system-wide processing on `output_uid` (or the default output).
    /// `eq_for_rate` builds the EQ once the device sample rate is known.
    pub fn start(
        output_uid: Option<&str>,
        buffer_frames: u32,
        shared: Arc<Shared>,
        eq_for_rate: impl FnOnce(f64) -> EqSettings,
    ) -> Result<Engine> {
        detach_hal_run_loop();
        let devices = list_output_devices();
        let uid = match output_uid {
            Some(u) => u.to_owned(),
            None => default_output_uid().ok_or_else(|| anyhow!("no default output device"))?,
        };
        let device =
            devices.into_iter().find(|d| d.uid == uid).ok_or_else(|| anyhow!("output device not found: {uid}"))?;

        // Our own process must be excluded from the tap, otherwise we would
        // capture (and mute) our own corrected output → feedback.
        let pid = std::process::id() as i32;
        let me: AudioObjectID = get_q(SYS, kAudioHardwarePropertyTranslatePIDToProcessObject, GLOBAL, Some(&pid))?;
        if me == 0 {
            bail!("could not resolve our own CoreAudio process object (needed to avoid feedback)");
        }

        // --- 1. Process tap, muted while we read it.
        // Preferred: tap only the audio every other process sends to the
        // headphone device's stream 0. It runs in the device's own IO cycle,
        // so the path adds ~2 buffers (≈5 ms @ 64 frames). The global stereo
        // mix tap is a fallback: it buffers ~40 ms more.
        let exclude = NSArray::from_retained_slice(&[NSNumber::numberWithUnsignedInt(me)]);
        let make_desc = |device_tap: bool| unsafe {
            let d = if device_tap {
                CATapDescription::initExcludingProcesses_andDeviceUID_withStream(
                    CATapDescription::alloc(),
                    &exclude,
                    &nsstr(&device.uid),
                    0,
                )
            } else {
                CATapDescription::initStereoGlobalTapButExcludeProcesses(CATapDescription::alloc(), &exclude)
            };
            d.setName(&nsstr("Flatline"));
            d.setPrivate(true);
            d.setMuteBehavior(CATapMuteBehavior::MutedWhenTapped);
            d
        };
        let mut device_tap = std::env::var("FL_TAP").as_deref() != Ok("global");
        let mut tap: AudioObjectID = 0;
        let mut desc = make_desc(device_tap);
        if device_tap && unsafe { AudioHardwareCreateProcessTap(Some(&desc), &mut tap) } != 0 {
            device_tap = false;
        }
        if !device_tap {
            desc = make_desc(false);
            check(unsafe { AudioHardwareCreateProcessTap(Some(&desc), &mut tap) }, "AudioHardwareCreateProcessTap")?;
        }
        let tap_uid = unsafe { desc.UUID().UUIDString() }.to_string();
        let tap_fmt: AudioStreamBasicDescription =
            get(tap, kAudioTapPropertyFormat, GLOBAL).unwrap_or(AudioStreamBasicDescription {
                mSampleRate: 0.0,
                mFormatID: 0,
                mFormatFlags: 0,
                mBytesPerPacket: 0,
                mFramesPerPacket: 0,
                mBytesPerFrame: 0,
                mChannelsPerFrame: 2,
                mBitsPerChannel: 0,
                mReserved: 0,
            });

        // --- 2. Private aggregate: headphones (clock master) + the tap.
        let agg = match create_aggregate(&device.uid, &tap_uid) {
            Ok(a) => a,
            Err(e) => {
                unsafe { AudioHardwareDestroyProcessTap(tap) };
                return Err(e);
            }
        };
        let cleanup = |agg, tap| unsafe {
            AudioHardwareDestroyAggregateDevice(agg);
            AudioHardwareDestroyProcessTap(tap);
        };

        let _ = set::<u32>(agg, kAudioDevicePropertyBufferFrameSize, GLOBAL, buffer_frames);
        let sample_rate: f64 = get(agg, kAudioDevicePropertyNominalSampleRate, GLOBAL).unwrap_or(48_000.0);
        let actual_frames: u32 = get(agg, kAudioDevicePropertyBufferFrameSize, GLOBAL).unwrap_or(buffer_frames);
        let device_latency_frames = get::<u32>(device.id, kAudioDevicePropertyLatency, OUTPUT).unwrap_or(0)
            + get::<u32>(device.id, kAudioDevicePropertySafetyOffset, OUTPUT).unwrap_or(0);

        // --- 3. Locate the tap among the aggregate's input buffers (sub-device
        // inputs come first, taps last) and the stereo pair among the outputs.
        let in_layout = stream_layout(agg, INPUT);
        let out_layout = stream_layout(agg, OUTPUT);
        if in_layout.is_empty() || out_layout.is_empty() {
            cleanup(agg, tap);
            bail!("aggregate device has no tap input or no output (in {in_layout:?}, out {out_layout:?})");
        }
        let mut tap_buf = in_layout.len() - 1;
        let mut acc = in_layout[tap_buf];
        while acc < tap_fmt.mChannelsPerFrame.max(1) && tap_buf > 0 {
            tap_buf -= 1;
            acc += in_layout[tap_buf];
        }
        let stereo: [u32; 2] = get(device.id, kAudioDevicePropertyPreferredChannelsForStereo, OUTPUT).unwrap_or([1, 2]);
        let locate = |ch: u32| -> (usize, usize) {
            let mut base = 1;
            for (i, &n) in out_layout.iter().enumerate() {
                if ch >= base && ch < base + n {
                    return (i, (ch - base) as usize);
                }
                base += n;
            }
            (0, 0)
        };
        let (out_l, out_r) = (locate(stereo[0]), locate(stereo[1]));
        let tap_chans = tap_fmt.mChannelsPerFrame as usize;
        let tap_ch = if device_tap && out_l.0 == 0 && out_r.0 == 0 && out_l.1.max(out_r.1) < tap_chans {
            [out_l.1, out_r.1]
        } else {
            [0, 1]
        };
        let passthrough = device_tap && tap_chans > 2 && out_layout.first() == Some(&(tap_chans as u32));

        // --- 4. Real-time state + IO proc.
        let eq = eq_for_rate(sample_rate);
        let (params_tx, params_rx) = rtrb::RingBuffer::new(16);
        let (scope_tx, scope_rx) = rtrb::RingBuffer::new(SCOPE_CAPACITY);
        let state = Box::into_raw(Box::new(IoState {
            proc: Processor::new(eq, sample_rate),
            params: params_rx,
            scope: scope_tx,
            shared: shared.clone(),
            tap_buf,
            tap_ch,
            passthrough,
            out_l,
            out_r,
            scratch: vec![[0.0; 2]; SCRATCH_FRAMES].into_boxed_slice(),
            dry: vec![0.0; SCRATCH_FRAMES].into_boxed_slice(),
            sample_period: 1.0 / sample_rate,
        }));
        unsafe { (*state).proc.set_bypass(shared.bypass.load(Relaxed)) };

        let mut proc_id: AudioDeviceIOProcID = None;
        let st =
            unsafe { AudioDeviceCreateIOProcID(agg, Some(io_proc), state as *mut c_void, NonNull::from(&mut proc_id)) };
        if st != 0 {
            cleanup(agg, tap);
            drop(unsafe { Box::from_raw(state) });
            check(st, "AudioDeviceCreateIOProcID")?;
        }
        disable_unused_inputs(agg, proc_id, &in_layout, tap_buf);
        let st = unsafe { AudioDeviceStart(agg, proc_id) };
        if st != 0 {
            unsafe { AudioDeviceDestroyIOProcID(agg, proc_id) };
            cleanup(agg, tap);
            drop(unsafe { Box::from_raw(state) });
            check(st, "AudioDeviceStart")?;
        }

        Ok(Engine {
            agg,
            tap,
            proc_id,
            state,
            params: params_tx,
            scope: scope_rx,
            info: EngineInfo {
                device,
                sample_rate,
                buffer_frames: actual_frames,
                tap_channels: tap_fmt.mChannelsPerFrame,
                out_channels: (stereo[0], stereo[1]),
                device_latency_frames,
                agg_input_latency: (
                    get::<u32>(agg, kAudioDevicePropertyLatency, INPUT).unwrap_or(0),
                    get::<u32>(agg, kAudioDevicePropertySafetyOffset, INPUT).unwrap_or(0),
                    get_ids(agg, kAudioDevicePropertyStreams, INPUT)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|s| get::<u32>(s, kAudioStreamPropertyLatency, GLOBAL).unwrap_or(0))
                        .collect(),
                ),
                in_layout,
                out_layout,
                tap_buf,
                tap_mode: if device_tap { "device tap" } else { "global mix" },
            },
        })
    }

    /// Sends new EQ settings to the audio thread (lock-free; applied with a crossfade).
    pub fn send_eq(&mut self, eq: EqSettings) {
        let _ = self.params.push(eq);
    }

    /// Current nominal rate of the running device (to detect external changes).
    pub fn current_sample_rate(&self) -> Option<f64> {
        get(self.agg, kAudioDevicePropertyNominalSampleRate, GLOBAL).ok()
    }

    /// Whether CoreAudio reports the aggregate device as running.
    pub fn is_running(&self) -> bool {
        get::<u32>(self.agg, kAudioDevicePropertyDeviceIsRunning, GLOBAL).map(|v| v != 0).unwrap_or(false)
    }

    /// Whether the output device is still connected.
    pub fn device_alive(&self) -> bool {
        get::<u32>(self.info.device.id, kAudioDevicePropertyDeviceIsAlive, GLOBAL).map(|v| v != 0).unwrap_or(false)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        unsafe {
            AudioDeviceStop(self.agg, self.proc_id);
            AudioDeviceDestroyIOProcID(self.agg, self.proc_id);
            AudioHardwareDestroyAggregateDevice(self.agg);
            AudioHardwareDestroyProcessTap(self.tap);
            drop(Box::from_raw(self.state));
        }
    }
}

fn create_aggregate(output_uid: &str, tap_uid: &str) -> Result<AudioObjectID> {
    let one = NSNumber::numberWithInt(1);
    let zero = NSNumber::numberWithInt(0);
    let out_uid = nsstr(output_uid);
    let sub = NSDictionary::<NSString, AnyObject>::from_slices(
        &[&*key(kAudioSubDeviceUIDKey), &*key(kAudioSubDeviceDriftCompensationKey)],
        &[as_obj(&*out_uid), as_obj(&*zero)],
    );
    let tap = NSDictionary::<NSString, AnyObject>::from_slices(
        &[&*key(kAudioSubTapUIDKey), &*key(kAudioSubTapDriftCompensationKey)],
        &[as_obj(&*nsstr(tap_uid)), as_obj(&*one)],
    );
    let subs = NSArray::from_retained_slice(&[sub]);
    let taps = NSArray::from_retained_slice(&[tap]);
    let agg_uid = nsstr(&format!("{AGG_UID_PREFIX}.{}", objc2_foundation::NSUUID::new().UUIDString()));
    let name = nsstr("Flatline");
    let desc = NSDictionary::<NSString, AnyObject>::from_slices(
        &[
            &*key(kAudioAggregateDeviceUIDKey),
            &*key(kAudioAggregateDeviceNameKey),
            &*key(kAudioAggregateDeviceIsPrivateKey),
            &*key(kAudioAggregateDeviceIsStackedKey),
            &*key(kAudioAggregateDeviceMainSubDeviceKey),
            &*key(kAudioAggregateDeviceSubDeviceListKey),
            &*key(kAudioAggregateDeviceTapListKey),
            &*key(kAudioAggregateDeviceTapAutoStartKey),
        ],
        &[
            as_obj(&*agg_uid),
            as_obj(&*name),
            as_obj(&*one),
            as_obj(&*zero),
            as_obj(&*out_uid),
            as_obj(&*subs),
            as_obj(&*taps),
            as_obj(&*one),
        ],
    );
    // NSDictionary is toll-free bridged with CFDictionary.
    let cf: &CFDictionary = unsafe { &*(Retained::as_ptr(&desc) as *const CFDictionary) };
    let mut agg: AudioObjectID = 0;
    check(
        unsafe { AudioHardwareCreateAggregateDevice(cf, NonNull::from(&mut agg)) },
        "AudioHardwareCreateAggregateDevice",
    )?;
    Ok(agg)
}

/// Turn off the headphone interface's own inputs for our IO proc so macOS
/// does not open (and flag) the microphone; we only need the tap.
fn disable_unused_inputs(agg: AudioObjectID, proc_id: AudioDeviceIOProcID, layout: &[u32], tap_buf: usize) {
    if tap_buf == 0 {
        return;
    }
    let a = addr(kAudioDevicePropertyIOProcStreamUsage, INPUT);
    // struct AudioHardwareIOProcStreamUsage { void* mIOProc; UInt32 mNumberStreams; UInt32 mStreamIsOn[]; }
    let n = layout.len();
    let size = 8 + 4 + 4 * n;
    let mut buf = vec![0u64; size.div_ceil(8)];
    unsafe {
        let base = buf.as_mut_ptr() as *mut u8;
        *(base as *mut *const c_void) = proc_id.map_or(ptr::null(), |f| f as *const c_void);
        let mut sz = size as u32;
        if AudioObjectGetPropertyData(
            agg,
            NonNull::from(&a),
            0,
            ptr::null(),
            NonNull::from(&mut sz),
            NonNull::new_unchecked(base as *mut c_void),
        ) != 0
        {
            return;
        }
        let count = (*(base.add(8) as *const u32) as usize).min(n);
        let on = base.add(12) as *mut u32;
        for i in 0..count {
            *on.add(i) = (i >= tap_buf) as u32;
        }
        AudioObjectSetPropertyData(
            agg,
            NonNull::from(&a),
            0,
            ptr::null(),
            sz,
            NonNull::new_unchecked(base as *mut c_void),
        );
    }
}

// ------------------------------------------------------- IO callback ----

unsafe extern "C-unwind" fn io_proc(
    _device: AudioObjectID,
    _now: NonNull<AudioTimeStamp>,
    input: NonNull<AudioBufferList>,
    input_time: NonNull<AudioTimeStamp>,
    output: NonNull<AudioBufferList>,
    output_time: NonNull<AudioTimeStamp>,
    client: *mut c_void,
) -> i32 {
    if client.is_null() {
        return 0;
    }
    let st = unsafe { &mut *(client as *mut IoState) };
    let t0 = Instant::now();
    let frames = catch_unwind(AssertUnwindSafe(|| unsafe { st.render(input, output) })).unwrap_or(0);
    if frames > 0 {
        let budget = frames as f64 * st.sample_period;
        st.shared.load.store_max((t0.elapsed().as_secs_f64() / budget) as f32);
        st.shared.frames.store(frames as u32, Relaxed);
        let offset = unsafe { output_time.as_ref().mSampleTime - input_time.as_ref().mSampleTime };
        st.shared.io_offset.store(offset as f32);
    }
    st.shared.callbacks.fetch_add(1, Relaxed);
    0
}

#[inline]
unsafe fn buffers<'a>(list: NonNull<AudioBufferList>) -> &'a mut [AudioBuffer] {
    let l = list.as_ptr();
    unsafe { std::slice::from_raw_parts_mut((*l).mBuffers.as_mut_ptr(), (*l).mNumberBuffers as usize) }
}

/// The samples of one CoreAudio buffer as an interleaved `f32` slice, plus its
/// channel count.
///
/// The `&mut` derived from `&AudioBuffer` is intentional: the buffer struct is
/// only a descriptor, the samples belong to CoreAudio for the duration of the
/// IO callback, and each buffer is turned into a slice at most once at a time.
#[inline]
#[allow(clippy::mut_from_ref)]
fn samples(b: &AudioBuffer) -> Option<(&mut [f32], usize)> {
    if b.mData.is_null() || b.mNumberChannels == 0 {
        return None;
    }
    let len = b.mDataByteSize as usize / 4;
    Some((unsafe { std::slice::from_raw_parts_mut(b.mData as *mut f32, len) }, b.mNumberChannels as usize))
}

impl IoState {
    unsafe fn render(&mut self, input: NonNull<AudioBufferList>, output: NonNull<AudioBufferList>) -> usize {
        let ins = unsafe { buffers(input) };
        let outs = unsafe { buffers(output) };

        let frames = outs.first().and_then(samples).map(|(s, ch)| s.len() / ch).unwrap_or(0);
        for b in outs.iter_mut() {
            if let Some((s, _)) = samples(b) {
                s.fill(0.0);
            }
        }

        if self.passthrough
            && let (Some((src, sch)), Some((dst, dch))) =
                (ins.get(self.tap_buf).and_then(samples), outs.first().and_then(samples))
            && sch == dch
        {
            let n = src.len().min(dst.len());
            dst[..n].copy_from_slice(&src[..n]);
        }

        let mut latest = None;
        while let Ok(eq) = self.params.pop() {
            latest = Some(eq);
        }
        if let Some(eq) = latest {
            self.proc.set_eq(eq);
        }
        self.proc.set_bypass(self.shared.bypass.load(Relaxed));

        // Tap input: interleaved stereo in one buffer, or two mono buffers.
        let tap_l = ins.get(self.tap_buf).and_then(samples);
        let tap_r = if tap_l.as_ref().is_some_and(|(_, ch)| *ch == 1) {
            ins.get(self.tap_buf + 1).and_then(samples)
        } else {
            None
        };

        let mut done = 0;
        let (mut pin, mut pout) = ([0f32; 2], [0f32; 2]);
        let mut clips = 0u64;
        while done < frames {
            let n = (frames - done).min(self.scratch.len());
            let buf = &mut self.scratch[..n];
            for (i, fr) in buf.iter_mut().enumerate() {
                let f = done + i;
                *fr = match (&tap_l, &tap_r) {
                    (Some((l, 1)), Some((r, _))) => {
                        [l.get(f).copied().unwrap_or(0.0), r.get(f).copied().unwrap_or(0.0)]
                    }
                    (Some((s, 1)), None) => {
                        let v = s.get(f).copied().unwrap_or(0.0);
                        [v, v]
                    }
                    (Some((s, ch)), _) => [
                        s.get(f * ch + self.tap_ch[0]).copied().unwrap_or(0.0),
                        s.get(f * ch + self.tap_ch[1]).copied().unwrap_or(0.0),
                    ],
                    _ => [0.0, 0.0],
                };
                pin[0] = pin[0].max(fr[0].abs());
                pin[1] = pin[1].max(fr[1].abs());
            }
            for (d, fr) in self.dry.iter_mut().zip(buf.iter()) {
                *d = 0.5 * (fr[0] + fr[1]);
            }

            self.proc.process(buf);

            for (i, fr) in buf.iter_mut().enumerate() {
                for c in 0..2 {
                    let a = fr[c].abs();
                    pout[c] = pout[c].max(a);
                    if a > 1.0 {
                        clips += 1;
                        fr[c] = fr[c].clamp(-1.0, 1.0);
                    }
                }
                let f = done + i;
                for (c, (b, ch)) in [self.out_l, self.out_r].into_iter().enumerate() {
                    if let Some((s, nch)) = outs.get(b).and_then(samples)
                        && let Some(x) = s.get_mut(f * nch + ch)
                    {
                        *x = fr[c];
                    }
                }
                let _ = self.scope.push([self.dry[i], 0.5 * (fr[0] + fr[1])]);
            }
            done += n;
        }

        for c in 0..2 {
            self.shared.peak_in[c].store_max(pin[c]);
            self.shared.peak_out[c].store_max(pout[c]);
        }
        if clips > 0 {
            self.shared.clips.fetch_add(clips, Relaxed);
        }
        frames
    }
}
