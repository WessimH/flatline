# L'équipe Flatline

Ce fichier est la règle commune de tous les agents de `.claude/agents/`.
Chaque agent le lit avant de travailler.

## Le projet en bref

Flatline corrige le son d'un casque sur macOS, pour tout le système.
L'appli capte l'audio du Mac, applique un égaliseur calculé pour le casque, et renvoie le son vers le casque.
Elle est écrite en Rust, avec une interface egui (design « Horizon »).

Pour comprendre le code : `src/lib.rs`, puis les dossiers `src/engine` (audio), `src/dsp` (calcul de l'égaliseur), `src/catalog.rs` (mesures de casques AutoEq) et `src/ui` (interface).

## Qui fait quoi

L'organigramme avec les noms et les postes est dans `docs/equipe/organigramme.md`.


| Agent | Rôle en une phrase | Peut modifier |
|---|---|---|
| `po` | Décide quoi faire et pourquoi, fixe les priorités. | Linear |
| `ba` | Transforme une idée en tickets clairs et prépare le plan d'équipe. | Linear |
| `architect` | Choisit la structure du code avant qu'on code. | rien (avis) |
| `sound-engineer` | Vérifie que la correction sonne juste, chiffres à l'appui. | rien (avis) |
| `dev` | Écrit le code de l'appli et ses tests. | `src/`, `Cargo.toml`, `Cargo.lock` |
| `web-dev` | Code le site vitrine. | `site/` |
| `qa` | Teste, fait des captures, trouve les bugs. | Linear (bugs) |
| `devops` | Build, signature, CI, paquet de release. | `scripts/`, `.github/` |
| `challenger` | Remet chaque décision en question. | rien (avis) |
| `minimalist` | Empêche le code et les fonctions inutiles. | rien (avis) |
| `ux-ui` | Parcours, écrans, états, maquettes. | `design/` |
| `graphic-designer` | Logo, icônes, visuels. | `design/`, `assets/` (images), `site/assets/` |
| `marketing` | Positionnement, textes de vente, suggestions de fonctions. | `README.md` (haut de page), textes de `site/`, tickets « Suggestion » dans Linear |
| `com` | Notes de version et annonces. | `release-notes/` |
| `release` | Numéro de version, changelog, tag, GitHub Release. | `CHANGELOG.md`, version dans `Cargo.toml`, tags, releases |
| `doc` | La doc GitBook, simple à lire pour un humain. | `docs/` (sauf `docs/equipe/`) |
| `hr` | Recrute (crée) ou retire des agents, et tient le budget de tokens. | `.claude/agents/`, le tableau ci-dessus |

Un agent ne modifie jamais un fichier hors de sa colonne « Peut modifier ».
S'il voit un problème ailleurs, il le signale dans son rapport.

## Comment on travaille ensemble

Un agent ne peut pas en appeler un autre.
Le PO et le BA font donc le plan : quel agent fait quoi, dans quel ordre, avec quel ticket.
La session principale lit ce plan et appelle les agents un par un.

Circuits habituels :

- **Nouvelle fonction** : `po` + `ba` → `architect` → `sound-engineer` (si ça touche au son) → `dev` → `minimalist` → `qa` → `doc`.
- **Bug** : `qa` (reproduit) → `dev` (corrige) → `qa` (vérifie).
- **Release** : `qa` → `challenger` → `com` (texte) → `release` (publie) → `marketing` (relais README et site).
- **Idée de fonction** : `marketing` (ticket « Suggestion » avec preuves) → `po` (garde, met de côté ou refuse) → `ba` (tickets) → circuit « nouvelle fonction ».
- **Besoin d'un nouveau rôle** : `po` constate le manque → `hr` propose l'agent → Wessim valide → `hr` le crée.
- **Site vitrine** : `marketing` (textes) → `ux-ui` + `graphic-designer` (maquette) → `web-dev` → `qa`.

`challenger` et `minimalist` peuvent être appelés à n'importe quelle étape.

## Comment le travail est organisé

On reprend le fonctionnement d'une entreprise. Le travail a cinq niveaux, du plus large au plus précis :

1. **Initiative** : un grand objectif sur plusieurs mois. Exemple : « Sortir Flatline 1.0 en public ».
2. **Projet** : un chantier qui sert une initiative, avec une date cible. Exemple : « Site vitrine ».
3. **Milestone** : une étape visible d'un projet. Exemple : « maquette validée », puis « site en ligne ».
4. **Ticket** : une tâche qu'un seul agent peut finir. Exemple : « FLA-12 : coder la page d'accueil ».
5. **Cycle** : le sprint de deux semaines, c'est-à-dire les tickets qu'on fait maintenant.

Qui fait quoi :
- Le **PO** crée les initiatives et les projets, fixe les dates et choisit le contenu de chaque cycle.
- Le **PO et le BA** découpent un projet en milestones.
- Le **BA** écrit les tickets et rattache chacun à son projet et à sa milestone.
- Tout ticket appartient à un projet. Un ticket sans projet est une idée à trier (Backlog).

Chaque projet a une description courte : pourquoi on le fait, à quelle initiative il sert, comment on sait qu'il est fini.
Le PO écrit un point d'avancement sur le projet (« status update ») à chaque fin de milestone.

## Le budget : les tokens

Le budget de l'équipe, ce sont les tokens de Wessim. Ils se rechargent **toutes les 5 heures**.
Quand ils sont épuisés, plus aucun agent ne travaille jusqu'à la recharge.

Ce que ça implique pour tout le monde :
- **Chaque appel d'agent coûte.** Opus coûte le plus cher, puis Sonnet, puis Haiku. On appelle seulement les agents utiles à la tâche, pas tout le circuit par habitude.
- **Un agent lit seulement ce dont il a besoin.** Pas de lecture de tout le dépôt « pour comprendre » : il part du ticket et des fichiers qu'il cite.
- **Le travail se découpe en morceaux de moins de 5 heures.** Un ticket doit pouvoir être fini dans une seule fenêtre.
- **La trace « Pour reprendre » passe avant tout.** Si le budget baisse, l'agent s'arrête proprement, écrit où il en est, et rend la main. Mieux vaut un travail à moitié fait mais bien noté qu'un travail perdu.
- Le PO dimensionne chaque plan : il indique les agents à appeler et leur modèle, et garde les agents Opus pour ce qui en a vraiment besoin.

## Linear

Équipe **FlatLine**, clé **FLA** (tickets `FLA-12`…).
Le PO crée les projets et fixe les priorités, le BA écrit les tickets, le QA ouvre les bugs, le Marketing propose des idées (libellé « Suggestion »).
Les autres agents citent le numéro du ticket dans leur rapport.

## Reprendre le travail à tout moment

Wessim doit pouvoir lire n'importe quel ticket et reprendre le travail seul, sans agent.
Le ticket Linear est donc **la mémoire du projet**. Rien d'important ne reste seulement dans une conversation.

### Le modèle de ticket (écrit par le BA, en français)

```markdown
## Pourquoi
Le problème, vu par l'utilisateur, en deux phrases.

## Ce qu'on veut
Ce que l'utilisateur pourra faire une fois le ticket fini.

## Critères d'acceptation
- [ ] Une phrase vérifiable par critère (« Quand je débranche le casque, la carte "Disconnected" s'affiche. »)

## Comment ça marche aujourd'hui
Les fichiers et fonctions concernés, avec leur rôle (`src/ui/app.rs`, fonction `route()` : choisit si on corrige ou si on met en pause).

## Hors périmètre
Ce qu'on ne fait pas dans ce ticket.

## Pour reprendre
- **État :** pas commencé / en cours / à tester / fini.
- **Fait :** …
- **Reste à faire :** la prochaine étape, assez précise pour la faire soi-même.
- **Comment vérifier :** les commandes ou les écrans à regarder.
```

### La trace de fin de tâche (tous les agents)

Quand un agent termine sa part, il ajoute un **commentaire** au ticket :

```markdown
**[nom de l'agent]** · ce que j'ai fait en une phrase
- Fichiers touchés : `src/…` (fonction `…`)
- Vérifié : tests verts, capture de l'écran X
- Pas vérifié : …
- Suite : agent suivant et ce qu'il doit faire
```

Puis il met à jour la section **Pour reprendre** et le statut du ticket.

### Les statuts Linear

- **Backlog** : idée gardée, pas encore planifiée.
- **Todo** : prêt à faire (critères écrits).
- **In Progress** : quelqu'un travaille dessus.
- **In Review** : fini, en test par le QA.
- **Done** : critères vérifiés.
- **Canceled** : abandonné (avec la raison en commentaire).

## Langues

- Avec Wessim : **français**.
- Tout ce qui est public : **anglais** (code, commentaires, commits, README, notes de version, site).
- La doc GitBook (`docs/`) : français.

## Écrire pour des humains

Ces règles valent pour tout texte : rapports, tickets, doc, notes de version.
Elles viennent de l'article « Why is AI-generated language so hard to understand? » (arXiv 2610.29162).

- Une idée par phrase. Des phrases courtes.
- Sujet, verbe, complément, dans cet ordre. Le sujet et son verbe restent proches.
- Dire qui fait l'action. « L'appli calcule l'égaliseur », pas « le calcul est effectué ».
- Pas d'objet qui « décide » ou « pense » comme une personne.
- Pas d'empilement de noms. « Le code qui choisit la sortie audio », pas « gestionnaire de routage de sortie ».
- Des mots courants. Un mot technique est expliqué la première fois.
- Un exemple concret pour chaque idée importante.
- Pas trop court non plus : une phrase complète vaut mieux qu'un fragment.
- Un schéma quand il aide à comprendre (voir l'agent `doc`).

## Commandes utiles

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test --release
cargo clippy --release --all-targets -- -D warnings
./scripts/bundle.sh                       # construit dist/Flatline.app
open -W -n --env FL_DEMO=1 --env FL_STATE=live --env FL_SHOT=/tmp/shot.png dist/Flatline.app
```

`FL_STATE` accepte : `live`, `bypass`, `disconnected`, `changed`, `blocked`, `error`, `asleep`, `switcher`, `settings`, `access`, `pick`, `sound`.
Sans `FL_STATE`, `FL_DEMO=1` ouvre l'écran de bienvenue. Une seule instance de Flatline peut tourner : la quitter avant.
Journal de l'appli : `~/Library/Logs/Flatline/flatline.log`.

## Règles de git

Aucun agent ne commite, ne pousse ou ne publie sans l'accord explicite de Wessim.

### Relier le code aux tickets

Linear est relié au dépôt GitHub. Il rattache tout seul une branche, un commit ou une pull request à un ticket quand son identifiant (`FLA-12`) y apparaît.

- **Une branche par ticket**, créée depuis `main`. Son nom est celui que Linear propose (champ « branch name » du ticket, par exemple `wessim/fla-12-pause-on-unplug`). Sinon : `fla-12-description-courte`.
- **Chaque commit cite le ticket** au début du message : `FLA-12: pause correction when headphones are unplugged`.
- **La pull request** a le ticket dans son titre (`FLA-12: …`) et `Closes FLA-12` dans sa description. Linear passe alors le ticket en « In Review » à l'ouverture et en « Done » à la fusion.
- Un changement sans ticket (petite correction) peut aller sur `main` directement, avec l'accord de Wessim.

Exemple complet :

```bash
git switch -c wessim/fla-12-pause-on-unplug main
git commit -m "FLA-12: pause correction when headphones are unplugged"
git push -u origin wessim/fla-12-pause-on-unplug
gh pr create --title "FLA-12: Pause correction on unplug" --body "Closes FLA-12"
```
