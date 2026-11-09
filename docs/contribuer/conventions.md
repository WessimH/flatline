# Conventions de code et de commit

Cette page fixe comment on nomme les choses dans le code, et comment on écrit un message de commit.
Tout le monde la suit : les humains comme les agents.

## Nommer dans le code

On suit les [règles de nommage officielles de Rust](https://rust-lang.github.io/api-guidelines/naming.html) (les « API Guidelines »).
Voici l'essentiel, avec des exemples tirés de Flatline.

### La casse

La casse dépend de ce qu'on nomme.

| Ce qu'on nomme | Casse | Exemple dans Flatline |
|---|---|---|
| Type (`struct`, `enum`), trait | `UpperCamelCase` | `FlatlineApp`, `EqSettings`, `Status` |
| Variante d'enum | `UpperCamelCase` | `Status::NoOutput`, `Form::OverEar` |
| Fonction, méthode | `snake_case` | `start_engine`, `route_decision` |
| Variable, champ, paramètre | `snake_case` | `headphone_key`, `buffer_frames` |
| Module, fichier `.rs` | `snake_case` | `coreaudio.rs`, `sim.rs` |
| Constante, `static` | `SCREAMING_SNAKE_CASE` | `SETTINGS_VERSION`, `XFADE_SAMPLES` |
| Paramètre de type générique | une majuscule courte | `T`, `E` |
| Durée de vie | minuscule courte | `'a`, `'de` |

Un sigle compte comme un mot : on écrit `Uuid` et `Rta`, pas `UUID` ni `RTA`.
Dans `snake_case`, un sigle est tout en minuscules : `default_uid`, pas `default_UID`.

### Les conversions : `as_`, `to_`, `into_`

Le préfixe dit combien la conversion coûte et ce qu'elle fait de la valeur d'origine.

- `as_` : gratuit, on emprunte la valeur. Exemple : `as_str()`.
- `to_` : coûte un calcul ou une copie, la valeur d'origine reste. Exemple : `to_string()`.
- `into_` : consomme la valeur d'origine. Exemple : `into_bytes()`.

### Les accesseurs : pas de `get_`

Un accesseur porte le nom du champ, sans `get_`.

```rust
fn fs(&self) -> f64      // oui
fn get_fs(&self) -> f64  // non
```

On garde `get` seulement quand il n'y a qu'une chose évidente à lire, comme `Catalog::get()`.

### Les itérateurs

- Une méthode qui rend un itérateur s'appelle `iter()`, `iter_mut()` ou `into_iter()`.
- Le type de cet itérateur porte le nom de la méthode : `iter()` rend un `Iter`.

### L'ordre des mots

On garde le même ordre de mots partout. Un exemple tiré de la bibliothèque standard : `ParseIntError`, `ParseFloatError`, donc verbe, objet, puis `Error`.
Si on ajoute une erreur de lecture de scénario, on l'appelle `ParseScenarioError`, pas `ScenarioParseError`.

### Ce qui n'est pas du Rust

- Variables d'environnement de Flatline : `FL_` puis `SCREAMING_SNAKE_CASE`. Exemple : `FL_SCENARIO`, `FL_SHOT_SECS`.
- Fichiers de données et de scénario : `kebab-case`. Exemple : `output-appears.ron`.
- Agents : `kebab-case`. Exemple : `sound-engineer`.

## Écrire un message de commit

Un message de commit explique un changement à quelqu'un qui le lira dans six mois.
Il a trois parties : un titre, un corps, une signature.

### Le titre

- Il commence par l'identifiant du ticket Linear, puis deux-points : `FLA-28: `.
- Ensuite, une phrase à l'**impératif**, en anglais, qui commence par une majuscule. On écrit ce que fait le commit : « Add », « Fix », « Remove ».
- Il fait au plus 72 caractères, sans point final.

```text
FLA-28: Keep the next EQ until the current crossfade ends
```

Pas de ticket ? C'est une petite correction, et le titre commence directement par le verbe : `Fix rustdoc link to a private function`.

### Le corps

- Une ligne vide sépare le titre du corps.
- Le corps dit **pourquoi** on change, puis **quoi**. Le code montre déjà le comment.
- Lignes de 72 caractères au plus. Des tirets pour une liste.

```text
FLA-28: Keep the next EQ until the current crossfade ends

Moving a slider sends a new EQ every callback (~512 samples), shorter
than the 1024-sample crossfade. The old code ended the running fade at
once and clicked (jump of 0.91, limit 0.07).

- A second set_eq waits in `pending`; only the latest is kept.
- No allocation on the audio thread.
```

### La signature

Quand un agent a écrit le code, la dernière ligne le dit :

```text
Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
```

## Nommer une branche et une pull request

- **Branche :** le nom que Linear propose pour le ticket, par exemple `wharmel/fla-28-tests-de-dspprocessorrs-fondu-entre-deux`.
- **Pull request :** même titre que le commit principal, et `Closes FLA-28` dans la description. Linear ferme alors le ticket à la fusion.
