## LauncherCore  
> Ce lanceur, écrit en Rust, gère la plupart des processus clés du noyau de lancement de Minecraft, notamment la gestion des versions, la gestion des dépendances, le téléchargement des ressources, l'assemblage des paramètres de lancement et le démarrage des processus. Il est adapté pour être utilisé comme module backend pour un lanceur en ligne de commande (CLI) ou comme code de référence pour apprendre le processus de lancement de Minecraft.  

[![EN](https://img.shields.io/badge/English-Click-blue)](../README.md)
[![CN](https://img.shields.io/badge/简体中文-Click-blue)](./README_zh_CN.md)
[![FR](https://img.shields.io/badge/Français-Click-yellow)](./README_fr_FR.md)
![MIT](https://img.shields.io/badge/License-MIT-green)
![Rust](https://img.shields.io/badge/Rust-100%25-orange)

### Fonctionnalités :  
- **Interface en ligne de commande** : Construite avec Clap, simple et conviviale.  
- **Prend en charge toutes les versions officielles** : Récupère automatiquement la liste complète des versions de Minecraft et prend en charge l'installation et le lancement de n'importe quelle version officielle.  
- **Téléchargements de ressources automatiques** : Télécharge automatiquement les fichiers JAR du client, les dépendances (y compris les natives) et les fichiers de ressources (assets) sans intervention manuelle.  
- **Compatibilité multiplateforme** : Prend en charge les trois principales plateformes : Windows, Linux et macOS.  
- **Configuration flexible du chemin Java** : Permet de spécifier l'environnement d'exécution Java via les arguments de ligne de commande ou la variable d'environnement `JAVA_HOME`.  
- **Gestion locale des natives** : Extrait automatiquement les natives dans des répertoires dédiés pour chaque version, permettant la coexistence de plusieurs versions.  
- **Asynchrone et performant** : Utilise Tokio + Reqwest pour des téléchargements asynchrones haute performance.  
- **Gestion détaillée des erreurs** : Utilise anyhow/thiserror pour fournir des messages d'erreur complets à chaque étape.  

### Compilation :  
```bash  
git clone https://github.com/HuanMeng-official/LauncherCore.git  
cd LauncherCore  
cargo build --release  
```  
Après la compilation, l'exécutable se trouve dans ``target/release/mclc.exe`` ou ``target/release/mclc``.

### Utilisation :  
``mclc <COMMANDES> <OPTIONS>``  
| Commandes | Description |  
| --- | --- |  
| **list** | Lister les versions disponibles de Minecraft |  
| **install** | Installer une version de Minecraft |  
| **launch** | Lancer Minecraft |  
| **help** | Afficher ce message ou l'aide pour la(les) sous-commande(s) donnée(s) |  
| **login** | Se connecter à un compte Microsoft |  

| Options | Description |  
| --- | --- |  
| **-r, --runtime** | Définir le chemin de Java |  
| **-h, --help** | Afficher l'aide |  

*Par exemple :*  
En ligne :  
1. ``mclc login``  
2. ``mclc launcher <Version>``  

Hors ligne :  
1. ``mclc launcher <Version> --username <NomDuJoueur> --runtime "C:\Program Files\Javain\java.exe"``  

### Principe de fonctionnement :  
 - Appelle le [manifest officiel des versions Mojang](https://launchermeta.mojang.com/mc/game/version_manifest.json) pour récupérer la liste complète des versions prises en charge.  
 - Lors de l'installation, télécharge automatiquement le JAR client, les bibliothèques de dépendance (y compris les natives), l'index des assets et les fichiers d'assets, puis extrait automatiquement les natives.  
 - Lors du lancement, assemble automatiquement le classpath, les chemins des natives et tous les arguments requis, puis invoque Java pour démarrer le client Minecraft.  

### Structure du répertoire :  
```
Projet/
  ├── src/
  │    └── main.rs
  ├── Cargo.toml
  └── target/
        ├── debug/
        ├── .rust_info.json
        └── CACHEDIR.TAG
```

### FAQ :  
 - Java non trouvé ?  
Définissez la variable d'environnement JAVA_HOME ou spécifiez explicitement le chemin Java avec --runtime.  
 - Échec du lancement ou erreurs ?  
Vérifiez que les dépendances et les ressources ont bien été téléchargées, ou consultez la sortie détaillée des erreurs dans le terminal.  

### Contribuer :  
Issues et PR bienvenues ! Veuillez ouvrir une issue pour décrire votre idée avant de soumettre une pull request.  

### Licence :  
MIT