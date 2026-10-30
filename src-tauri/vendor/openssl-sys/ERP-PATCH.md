# Correctif de build local

Origine : crate openssl-sys 0.9.117, https://crates.io/crates/openssl-sys/0.9.117, sources et licences d'origine conservées.

Une seule modification de code : dans `build/main.rs`, décoder la sortie du préprocesseur par `String::from_utf8_lossy(&expanded)` au lieu de `String::from_utf8(expanded).unwrap()`.

Raison : MSVC émet certains chemins des directives `#line` avec la page de codes locale Windows. Un chemin contenant « bâtiment » provoque un panic UTF-8 avant la détection de version OpenSSL, même lorsque les bibliothèques natives sont correctement compilées. Les tokens de configuration réellement interprétés sont ASCII ; les chemins ne sont pas utilisés pour calculer la version.

Ce correctif n'altère ni les sources cryptographiques OpenSSL, ni leurs paramètres, ni les signatures. Il reste local jusqu'à disponibilité d'une correction amont à tester.
