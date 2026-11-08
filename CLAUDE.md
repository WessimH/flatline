# Flatline

L'équipe d'agents et ses règles sont dans `.claude/TEAM.md` (agents dans `.claude/agents/`).

Pour toute demande qui dépasse une petite correction :
1. Appelle `po` et `ba` : ils rendent le plan d'équipe (quel agent, dans quel ordre, quel ticket Linear FLA).
2. Suis ce plan en appelant les agents un par un (un agent ne peut pas en appeler un autre).
3. Après chaque agent, vérifie que sa trace de fin de tâche est dans le ticket Linear (sinon copie-la depuis son rendu).
4. Respecte les domaines de `.claude/TEAM.md` : chaque agent ne modifie que ses fichiers.

Parle français à Wessim ; tout ce qui est public est en anglais. Ne commite, ne pousse et ne publie rien sans son accord.
