# Flatline : carte du code

Guide pour s'y retrouver et modifier le code à la main : quel fichier fait quoi, où changer telle chose, comment tester.

## Vue d'ensemble

```text
Audio système ──► tap CoreAudio ──► EQ (biquads, 0 échantillon) ──► sortie (casque)
                     engine/                dsp/processor
Mesure du casque (catalog) + cible ──► dsp/fit (calcul de l'EQ) ──► envoyé au thread audio
Interface egui (ui/) : lit l'état, change les Settings, affiche courbes et états
```

Deux threads importants :
- **Thread audio** (callback CoreAudio, `engine/coreaudio.rs`) : ne fait que filtrer. Aucune allocation, aucun verrou. Il reçoit les nouveaux EQ par une file sans verrou (`rtrb`).
- **Thread UI** (`ui/app.rs`) : tout le reste. Le calcul d'EQ tourne sur un thread `designer` à part pour ne pas figer l'interface.

## Les fichiers

### Racine
| Fichier | Rôle |
|---|---|
| `src/main.rs` | Point d'entrée : options de ligne de commande (`--print-eq`, `--write-iconset`, `--background`, `--headless`…), verrou d'instance unique, taille de fenêtre (`with_inner_size`), icône. |
| `src/lib.rs` | Déclare les modules. |
| `src/log.rs` | `log::log("…")` écrit dans `~/Library/Logs/Flatline/flatline.log` (rotation à 1 Mo). |
| `src/login.rs` | Lancement à l'ouverture de session (LaunchAgent). |
| `src/profiles.rs` | Anciennes mesures intégrées (DT 770) et les 2 cibles fixes : `TARGETS[0]` Harman OE 2018, `TARGETS[1]` oratory1990 Optimum. |
| `src/catalog.rs` | Catalogue AutoEq (6 813 mesures) : `Catalog::get()`, `search()`, `find(key)`, `curve(e)`, `target(e)`, `target_name(e)`, `sparkline()`. Données dans `data/catalog.bin`. |
| `src/analyzer.rs` | Analyseur de spectre temps réel (FFT) : courbe grise du graphique et barres de la carte Output. |

### `src/dsp/` : le traitement
| Fichier | Rôle |
|---|---|
| `curve.rs` | `Curve` (fréquence, dB), lecture CSV, `log_grid()`, `band_mean()`. |
| `biquad.rs` | Filtres RBJ (shelf, peak), réponse en fréquence. |
| `fit.rs` | **Le cœur** : `design(mesure, cible, params, fs)` calcule les bandes d'EQ. `DesignParams` = basses, tilt, boost max, nombre de bandes. `build_target()` ajoute le shelf de basses à 105 Hz et le tilt. |
| `processor.rs` | `EqSettings` : l'EQ prêt pour le thread audio (préampli, gain de bypass égalisé). |

### `src/engine/` : l'audio macOS
| Fichier | Rôle |
|---|---|
| `mod.rs` | `Shared` : compteurs atomiques partagés UI/audio (pics, bypass, callbacks, clips, charge). |
| `coreaudio.rs` | `Engine::start()` : crée le *process tap* + un agrégat privé + l'IOProc. `list_output_devices()`, `default_output_uid()`. **Ne pas remettre** `kAudioAggregateDeviceTapAutoStartKey` à 1 (sinon BLOCKED quand rien ne joue). |

### `src/ui/` : l'interface
| Fichier | Rôle |
|---|---|
| `theme.rs` | **Couleurs** (`NIGHT`, `MILK`, `MIST`, `PULSE`, `HALO`, `SOLEIL`, `DEPTH`, `AMBER`, `ERROR`) et **polices** : `display()` Syne 800, `title()` Syne 700, `thin()` / `light()` / `body()` / `medium()` Outfit, `mono()`. Style egui global. |
| `aura.rs` | La lumière animée de fond. Positions, tailles et couleurs des halos dans `Aura::paint()` ; intensité selon l'humeur dans `update()` (`Mood::Live / Bypass / Paused`). |
| `widgets.rs` | Composants : `pill`, `tag`, `icon_button`, `power_button`, `status_pill`, `switch`, `hslider`, `amount_slider`, `dial`, `bars`, `heartbeat`, `paint_glass`, icônes (`enum Icon` + `draw_icon`), logo (`paint_symbol`, `logo`). |
| `graph.rs` | Le graphique de réponse en fréquence (`draw`), plage dB (`DB_TOP` / `DB_BOT`), info-bulle au survol. |
| `icon.rs` | Icône d'app et glyphe de barre de menu dessinés en code (géométrie du logo validé) + encodeur PNG. |
| `tray.rs` | Icône de barre de menu et son menu (Afficher / Correction / Quitter). |
| `power.rs` | Détection veille / réveil du Mac. |
| `app.rs` | **Le cerveau** : `Settings` (sauvegardés), `Status`, `FlatlineApp`, démarrage/arrêt du moteur, routage casque, `logic()` appelé à chaque image. |
| `app/views.rs` | **L'écran principal** et tous les états (voir plus bas). |
| `app/onboarding.rs` | **Première configuration** A1 à A4 et le sélecteur « Add headphones… ». |

### Données et scripts
| Fichier | Rôle |
|---|---|
| `data/catalog.bin` | Catalogue binaire, généré par `python3 -I scripts/build_catalog.py <clone AutoEq>`. Sources et cibles détaillées dans `data/catalog-sources.md`. |
| `assets/fonts/` | Syne et Outfit (licence OFL). |
| `assets/AppIcon.icns` | Régénéré par `bundle.sh` s'il est supprimé. |
| `scripts/bundle.sh` | Compile et crée `dist/Flatline.app` (signé « Flatline Dev » si l'identité existe). `--reset-permission` réinitialise la permission audio. |
| `scripts/make-signing-identity.sh` | Crée l'identité de signature stable (la permission survit aux recompilations). |
| `design/` | Maquettes FigCraft et logo VectorCraft (hors git). |

## Où changer quoi

### Écran principal (`src/ui/app/views.rs`)
| Je veux changer… | Fonction |
|---|---|
| La disposition générale (marges, hauteur des cartes, largeur de la colonne gauche) | `view()` |
| Logo, pastille d'état en haut, boutons casque / réglages | `header()` |
| Nom du casque en grand, sous-titre, presets, bouton power, « Correction on » | `hero()`, `title_line()` |
| Carte du graphique, légende, « 0.8 dB off target » | `chart_card()` |
| Cartes Amount / Bass / Latency / Output | `cards()` |
| Carte « casque débranché » | `disconnected_card()` |
| Carte « sortie changée » (AirPods…) | `output_changed_card()` |
| Carte erreur moteur | `error_card()` |
| Mac en veille | `asleep_card()` |
| Écran « bloqué par macOS » | `blocked_overlay()` |
| Liste « My headphones » | `switcher()`, `headphone_row()` |
| Panneau Réglages | `settings_sheet()`, `settings_body()` |
| Toast en bas, superposition glisser-déposer | `overlays()` |
| Boutons blancs / en verre | `primary_button()`, `secondary_button()` |

### Première configuration (`src/ui/app/onboarding.rs`)
| Écran | Fonction |
|---|---|
| Barre d'étapes 1-2-3 | `onb_header()` |
| A1 Bienvenue | `onb_welcome()` |
| A2 Autoriser l'audio système | `onb_access()` |
| A3 Choisir son casque (recherche, filtres, liste) | `onb_pick()` |
| Carte d'aperçu à droite de A3 | `pick_preview()` |
| A4 Choisir son son (4 cartes) | `onb_sound()` |
| Les presets Flat / Studio / Harman / Warm | `presets()` (en haut du fichier) |

L'étape courante est dans `Onboarding.step` ; `FlatlineApp.onb` vaut `Some(...)` tant que l'onboarding ou le sélecteur est affiché.

### Logique (`src/ui/app.rs`)
| Je veux changer… | Où |
|---|---|
| Ajouter un réglage sauvegardé | `struct Settings` + `Default` ; si besoin une migration dans `migrate()` et monter `SETTINGS_VERSION` |
| Casque par défaut | `DEFAULT_KEY` |
| Quel casque / quelle cible est utilisé | `entry()`, `headphone_curves()`, `target_name()`, `select_headphones()` |
| Pause quand le casque est débranché | `route()` + `route_decision()` (testé) |
| Délais de démarrage et de nouvelle tentative | `START_GRACE`, `retry_delay()` |
| Ce qui se passe à chaque image (santé du moteur, veille, barre de menu, Espace, Échap) | `logic()` |
| Charger un CSV déposé | `handle_drops()`, `load_measurement()` |
| Texte du menu de la barre de menu | `tray_status()` |
| Tailles de buffer proposées | `BUFFER_SIZES` |

## Cycle de vie

1. `new()` lit les réglages, les migre, crée la barre de menu, puis démarre le moteur (sauf si l'onboarding n'est pas fini).
2. `logic()` (chaque image) : envoie un calcul d'EQ si les réglages ont changé → reçoit le `Design` → envoie l'EQ au moteur. Toutes les 250 ms : liste des sorties, routage casque, santé du moteur (callbacks reçus ? sinon `Blocked` après 8 s, nouvelle tentative 10 s → 60 s).
3. `ui()` dessine `onboarding_view()` ou `view()`.

États (`enum Status`) : `Standby` (moteur coupé), `Starting`, `Live`, `Blocked` (macOS refuse ou rien ne passe), `Error`, `Asleep`, `Disconnected`, `OutputChanged`.

## Tester

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test --release
cargo clippy --release --all-targets -- -D warnings
./scripts/bundle.sh && open dist/Flatline.app
```

Capturer un écran sans toucher à tes réglages (`FL_DEMO=1` ne sauvegarde rien). Quitter Flatline d'abord, une seule instance peut tourner :

```bash
open -W -n --env FL_DEMO=1 --env FL_STATE=disconnected --env FL_SHOT=/tmp/shot.png --env FL_SHOT_SECS=3 dist/Flatline.app
```

`FL_STATE` : `live`, `bypass`, `disconnected`, `changed`, `blocked`, `error`, `asleep`, `switcher`, `settings`, ou une étape de setup `access`, `pick`, `sound` (sans `FL_STATE` : écran de bienvenue). Le fichier `.txt` à côté de la capture donne l'état du moteur.

En cas de problème audio : regarder d'abord `~/Library/Logs/Flatline/flatline.log`.

## Pièges connus

- **RON et `Option`** : les réglages sont sauvegardés en RON ; un champ `Option` attend `Some(x)`. Un champ mal typé fait échouer la lecture et **tous** les réglages repartent par défaut. Toujours tester un ancien fichier (voir le test `real_v3_file_loads_and_keeps_its_values`).
- **Thread audio** : rien qui alloue ou verrouille dans le callback de `coreaudio.rs`.
- **Couleurs / polices** : toujours passer par `theme.rs`, jamais de `Color32` en dur dans les vues.
- **`ui.painter()`** dessine dans l'ordre d'appel : ce qui est peint après passe au-dessus.
