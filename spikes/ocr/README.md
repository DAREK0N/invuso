# AP-S1 · Prototyp: Texterkennung vergleichen (Entscheidung 10.1)

Stand: 2026-10-05. Wegwerf-Prototyp außerhalb des Invuso-Workspace (eigene Crate, eigener `[workspace]`).
Beispielbilder, Modelle und Ergebnisse liegen lokal und werden nicht committet (`.gitignore`).

## 1. Vorgehen

| Schritt | Umsetzung |
|---|---|
| Beispiele | 11 frei lizenzierte Fotos/Scans von Wikimedia Commons (`samples/SOURCES.tsv`): 5 deutsch (Lidl ×2, Fressnapf, IKEA, Augustiner), 6 japanisch (Lotteria, Gyoza no Ohsho, Sukiya, Yoshinoya, Don Quijote-Kopf, verblasster Stempelbeleg). Eigene Belege des Nutzers fehlen noch. |
| Referenz | `ground_truth.json`: von Hand abgelesene Positionen (Name + Preis) und Gesamtsumme je Beleg. |
| Host | `src/main.rs`: `ocrs`, PaddleOCR (PP-OCRv5 mobile, PP-OCRv6 small/tiny) über `ort` **und** über `rten`. Gleiche Zeilen-Rekonstruktion für alle Engines. Rechner: Ryzen 7 5800X3D. |
| Android | Emulator `S26_Ultra` (x86_64, 4 Kerne). PaddleOCR/`ocrs` als für Android kompilierte Rust-Binärdatei per `adb shell`; ML Kit über eine vorübergehende Kotlin-Brücke in `invuso-app` (`mlkit/mlkit-bridge.patch`, danach wieder entfernt). |
| Wertung | `score.py`: Position zählt, wenn Name (≥ 50 % ähnlich) und Preis exakt in derselben Zeile stehen; Summe, wenn der Betrag als eigenes Token vorkommt. Leerzeichen nach dem Dezimaltrenner („9, 99“, typisch für ML Kit) werden für alle Engines toleriert. |

## 2. Ergebnisse

### 2.1 Erkennungsqualität

| Engine | DE Positionen | DE Summe | JA Positionen | JA Summe | JA Namens-Ähnlichkeit |
|---|---|---|---|---|---|
| `ocrs` 0.13.1 | 5/16 | 3/5 | 0/9 | (5/5)¹ | 0,27 |
| PP-OCRv5 mobile | 16/16 | 5/5 | 6/9 | 4/5 | 0,71 |
| **PP-OCRv6 small** | **16/16** | **5/5** | **7/9** (Emulator 8/9)² | **5/5** | **0,85** |
| PP-OCRv6 tiny | 16/16 | 5/5 | 0/9 (kein Kana im Wörterbuch) | 4/5 | 0,50 |
| PP-OCRv6 tiny-Detektor + small-Erkenner | 16/16 | 5/5 | 7/9 | 5/5 | 0,85 |
| ML Kit Latein | 16/16 | 5/5 | 1/9 | (5/5)¹ | 0,33 |
| ML Kit Japanisch (liest auch Latein) | 16/16 | 5/5 | 7/9 | 5/5 | 0,84 |

¹ Nur Ziffern erkannt; die Summe steht trotzdem als Zahl im Text.
² Abweichung durch leicht andere Bildskalierung (JPEG neu kodiert für den Emulator).

Beobachtungen:
- `ocrs`: keine Umlaute (`Grine`, `Gemusemais`), Preise oft verfälscht (`(.59`, `J.99`), Komma wird zu Punkt; kein Japanisch.
- PP-OCRv6 small: Lidl-Beleg praktisch fehlerfrei inkl. `ß`/Umlauten; Japanisch nahezu vollständig. Typischer Fehler: vereinfachtes chinesisches Zeichen statt japanischem (`单` statt `単`).
- ML Kit: Qualität auf Augenhöhe; setzt häufig ein Leerzeichen nach dem Komma (`1, 410`) und trennt Zeilen in Tabellen (Ohsho) schlechter.
- Gemeinsame Schwäche: verblasster Stempelbeleg (`ja_simple_2017`), dort scheitern alle an mindestens einer Zeile.
- `rten` und `ort` liefern mit denselben Paddle-Modellen **identische** Texte.

### 2.2 Geschwindigkeit (reine Erkennung, Mittelwert je Beleg)

| Engine | Host DE | Host JA | Emulator DE | Emulator JA |
|---|---|---|---|---|
| `ocrs` (rten) | 0,5 s | 0,3 s | 0,9 s | 0,3 s |
| PP-OCRv6 small, `ort` | 2,1 s | 0,5 s | – (kein ONNX Runtime für x86_64-Android) | – |
| PP-OCRv6 small, `rten`, Detektor max. 4000 px | 2,2 s | 0,6 s | 5,0 s | 1,0 s |
| **PP-OCRv6 small, `rten`, Detektor max. 1920 px** | **0,9 s** | **0,5 s** | **1,9 s** | **0,9 s** |
| ML Kit Latein | – | – | 2,0 s | 0,6 s |
| ML Kit Japanisch | – | – | 3,3 s | 1,1 s |

Die Detektor-Auflösung von 1920 px ändert die Qualität nicht (gleiche Wertung), halbiert aber die Zeit. Emulator-Zeiten laufen auf x86-Kernen des Hosts; auf einem ARM-Handy können beide Engines anders abschneiden (ML Kit nutzt TFLite, `rten` eigene SIMD-Kerne für aarch64). Eine Messung auf dem echten Handy fehlt noch.

### 2.3 Größe, Lizenz, Aufwand

| | `ocrs` | PaddleOCR via `rten` | PaddleOCR via `ort` | ML Kit (gebündelt) |
|---|---|---|---|---|
| Modelle | 12,2 MB (11,4 MB komprimiert) | v6 small: 31,1 MB (26,6 MB komprimiert); tiny-Det. + small-Erk.: 23 MB | wie `rten` | 3,5 MB |
| Laufzeit-Code (arm64) | `rten` (in der App-Binärdatei) | `rten`, reines Rust; Spike-Binärdatei gesamt 7,7 MB gestrippt (x86_64) | ONNX Runtime als native Bibliothek (+ ~15–20 MB, Schätzung) | `libmlkit_google_ocr_pipeline.so` 11,1 MB |
| Lizenz | MIT/Apache-2.0 | Code MIT/Apache-2.0, Modelle Apache-2.0 | ONNX Runtime MIT | proprietär (ML Kit Terms), Google-Abhängigkeit |
| Japanisch | nein | ja | ja | ja |
| Stufe (AGENTS.md 5) | 2 | 2 | 2 + native Bibliothek | 4 (Kotlin + JNI) |
| Android-Ziele | alle | alle (kompiliert ohne Weiteres für x86_64 und arm64) | vorgebaut nur `aarch64`; x86_64-Emulator nicht | alle |
| Web (WASM) später | ja | ja | eingeschränkt | nein |
| Integrationsaufwand | gering (fertige API) | mittel: Vor-/Nachverarbeitung selbst (im Spike ~300 Zeilen; fehlt noch: gedrehte Boxen, ggf. Zeilen-Orientierung) | wie `rten` + native Bibliothek bündeln | mittel: Kotlin-Brücke + JNI-Rückweg, Gradle-Abhängigkeit über `[android] gradle_dependencies` (funktioniert in dx 0.7.10) |

## 3. Empfehlung

**PaddleOCR PP-OCRv6 small, ausgeführt mit `rten` (reines Rust), hinter dem Trait `OcrEngine`.**

- Beste gemessene Qualität für Deutsch **und** Japanisch mit **einem** Modellsatz; ML Kit ist gleichwertig, aber nicht besser.
- Reines Rust (Stufe 2): keine Kotlin-Brücke, keine native Bibliothek, keine Google-Abhängigkeit, läuft auf jedem Android-Ziel, in Host-Tests/CI und später im Web.
- `rten` ist bereits die Engine von `ocrs`; die Paddle-Modelle laufen darin unverändert.
- Detektor-Eingabe auf max. 1920 px begrenzen: im Emulator schneller als ML Kit.

Offene Punkte, die zur Entscheidung gehören:
1. **Modellgröße ~31 MB** (> 5 MB, AGENTS.md 4.3/7.8): mitliefern oder per `OCR-05` bei Bedarf laden? Alternativ tiny-Detektor + small-Erkenner (23 MB, gleiche Qualität im Test).
2. Messung auf dem **echten Handy** und mit **deinen eigenen Belegen** (`samples/`), bevor AP-18 beginnt.
3. Fallback: ML Kit bliebe als zweite `OcrEngine`-Implementierung möglich, falls `rten` auf dem Handy zu langsam ist.

## 4. Reproduzieren

```sh
cd spikes/ocr
# Modelle: siehe Abschnitt 5; Bilder nach samples/
cargo run --release -- ocrs v6s-rten v6s-ort          # Ergebnisse in results/<engine>/
DET_MAX=1920 cargo run --release -- v6s-rten
python score.py ocrs v6s-rten                           # Wertung gegen ground_truth.json
```

Neben `<beleg>.txt` (zusammengesetzte Zeilen) schreibt jeder Lauf `<beleg>.tsv` mit den Rohfragmenten (`left top right bottom text`). Daraus stammen die anonymisierten Parser-Fixtures in `crates/invuso-core/tests/fixtures/receipts/` (AP-17; erzeugt mit `DET_MAX=1920 SAMPLE=de_ cargo run --release -- v6s-rten`).

Android (Emulator): `cargo build --release --target x86_64-linux-android` mit den NDK-Variablen `CC_x86_64_linux_android`, `AR_x86_64_linux_android`, `CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER`; Binärdatei, `models/` und `samples/` nach `/data/local/tmp/ocr`, dann `SPIKE_ROOT=/data/local/tmp/ocr ./ocr-spike v6s-rten`. ML Kit: `git apply mlkit/mlkit-bridge.patch`, Bilder nach `files/ocr-spike/samples`, Datei `files/ocr-spike/run` anlegen, App starten; `group_tsv.py` wandelt die Ergebnisse in Zeilen um.

## 5. Quellen

- `ocrs`-Modelle: `https://ocrs-models.s3-accelerate.amazonaws.com/text-{detection,recognition}.rten`
- PaddleOCR-ONNX-Modelle (RapidAI/RapidOCR auf ModelScope): `onnx/PP-OCRv6/{det,rec}/PP-OCRv6_{det,rec}_{small,tiny}.onnx`, `onnx/PP-OCRv5/...`; Wörterbücher unter `paddle/PP-OCRv6/rec/.../ppocrv6_dict.txt`
- ML Kit: `com.google.mlkit:text-recognition:16.0.1`, `com.google.mlkit:text-recognition-japanese:16.0.1`
- Crates: `ocrs` 0.13.1, `rten` 0.26.0, `ort` 2.0.0-rc.13, `image` 0.25.10
