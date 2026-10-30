# Logo fourni par l’utilisateur

app-original.svg : copie inchangée du fichier app.svg fourni le 29 octobre 2026.

src/assets/app-logo.svg : export SVG simple, lettrage converti en tracés avec la police Ubuntu Bold Italic installée sur le poste. Le dessin, les couleurs, la disposition et les proportions ne sont pas redessinés. L’interface et la génération des icônes utilisent cet export indépendant des polices.

Régénération après modification du SVG original (Inkscape installé) :

```powershell
& 'C:\Program Files\Inkscape\bin\inkscape.com' branding/app-original.svg --export-text-to-path --export-plain-svg --export-filename=src/assets/app-logo.svg
node node_modules/@tauri-apps/cli/tauri.js icon src/assets/app-logo.svg --output src-tauri/icons
```

Le projet contient les icônes déjà générées. Inkscape et Ubuntu ne sont donc pas nécessaires à l’exécution de l’application ou à une compilation sans modification du logo.
