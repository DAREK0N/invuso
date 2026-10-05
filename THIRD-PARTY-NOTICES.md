# Third-Party-Notices

Invuso enthält Software und Daten Dritter. Diese Datei listet alle Komponenten, die in die Android-App gebaut werden, mit Lizenz und Urheberrechtsvermerk, und danach die vollständigen Lizenztexte. Bei Crates mit mehreren Lizenzen zur Wahl („MIT OR Apache-2.0“) gilt die zuerst passende aus MIT, Apache-2.0, BSD, ISC, Zlib.

Erzeugt mit `python scripts/third-party/generate.py`; nicht von Hand bearbeiten.

## Gebündelte Komponenten

| Komponente | Version | Lizenz | Copyright |
|---|---|---|---|
| SQLite (über libsqlite3-sys) | 3.53.2 | Public Domain | – |
| Lucide Icons (über dioxus-free-icons) | 0.265.0 | ISC; Feather-Anteile MIT | Lucide Contributors 2022; Cole Bemis 2013-2022 |
| Tailwind CSS (erzeugtes Stylesheet) | 4.1.5 | MIT | Tailwind Labs, Inc. |
| Mozilla-Root-Zertifikate (über webpki-roots) | – | CDLA-Permissive-2.0 | – |
| PaddleOCR PP-OCRv6 small (Texterkennungsmodelle und Wörterbuch, ONNX über RapidOCR) | PP-OCRv6 | Apache-2.0 | PaddlePaddle Authors |

## Rust-Crates

| Crate | Version | Lizenz | Copyright |
|---|---|---|---|
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 | Copyright (C) Jonas Schievink <jonasschievink@gmail.com> |
| aho-corasick | 1.1.5 | Unlicense OR MIT | Copyright (c) 2015 Andrew Gallant |
| aligned | 0.4.3 | MIT OR Apache-2.0 | Copyright (c) 2017 Jorge Aparicio |
| aligned-vec | 0.6.4 | MIT | Copyright (c) 2022 sarah |
| anyhow | 1.0.104 | MIT OR Apache-2.0 | Copyright (c) David Tolnay |
| arc-swap | 1.9.2 | MIT OR Apache-2.0 | Copyright (c) 2017 arc-swap developers |
| arrayvec | 0.7.8 | MIT OR Apache-2.0 | Copyright (c) Ulrik Sverdrup "bluss" 2015-2023 |
| as-slice | 0.2.1 | MIT OR Apache-2.0 | Copyright (c) 2018 Jorge Aparicio |
| av-scenechange | 0.14.1 | MIT | Copyright (c) 2019 Multimedia and Rust |
| av1-grain | 0.2.5 | BSD-2-Clause | Copyright (c) 2022-2022, the rav1e contributors |
| avif-serialize | 0.8.9 | BSD-3-Clause | Copyright (c) 2020, Cloudflare, Inc. |
| base62 | 2.2.6 | MIT | Copyright (c) 2015 François Bernier |
| base64 | 0.22.1 | MIT OR Apache-2.0 | Copyright (c) 2015 Alice Maz |
| base64 | 0.23.1 | MIT OR Apache-2.0 | Copyright (c) 2025 Alice Maz, Marshall Pierce |
| bit_field | 0.10.3 | Apache-2.0/MIT | Copyright (c) 2016 Philipp Oppermann |
| bitflags | 1.3.2 | MIT/Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| bitflags | 2.13.2 | MIT OR Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| bitstream-io | 4.10.0 | MIT/Apache-2.0 | Copyright (c) 2017 Brian Langenberger |
| block-buffer | 0.10.4 | MIT OR Apache-2.0 | Copyright (c) 2018-2019 The RustCrypto Project Developers |
| borsh | 1.8.1 | MIT OR Apache-2.0 | Copyright 2019 Near |
| bstr | 1.13.1 | MIT OR Apache-2.0 | Copyright (c) 2018-2019 Andrew Gallant |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT | Copyright (c) 2019 Daniel "Lokathor" Gee. |
| byteorder | 1.5.0 | Unlicense OR MIT | Copyright (c) 2015 Andrew Gallant |
| byteorder-lite | 0.1.0 | Unlicense OR MIT | Copyright (c) 2015 Andrew Gallant |
| bytes | 1.12.1 | MIT | Copyright (c) 2018 Carl Lerche |
| cesu8 | 1.1.0 | Apache-2.0/MIT | Copyright (C) 2000-2010 Julian Seward.  All rights; Copyright (c) 2003-2013 University of Illinois at; Copyright (c) 2009-2014 by the contributors listed in |
| cfb | 0.7.3 | MIT | Copyright (c) 2017 Matthew D. Steele |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 | Copyright (c) 2014 Alex Crichton |
| color_quant | 1.1.0 | MIT | Copyright (c) 2016 PistonDevelopers |
| combine | 4.6.8 | MIT | Copyright (c) 2015 Markus Westerlind |
| const-serialize | 0.7.2 | MIT OR Apache-2.0 | Copyright (c) Evan Almloff |
| const-serialize | 0.8.0-alpha.0 | MIT OR Apache-2.0 | Copyright (c) Evan Almloff |
| const_format | 0.2.36 | Zlib | Copyright (c) 2020 Matias Rodriguez. |
| cookie | 0.18.2 | MIT OR Apache-2.0 | Copyright 2017 Sergio Benitez; Copyright 2014 Alex Chricton; Copyright (c) 2017 Sergio Benitez |
| cpufeatures | 0.2.17 | MIT OR Apache-2.0 | Copyright (c) 2020-2025 The RustCrypto Project Developers |
| crc32fast | 1.5.2 | MIT OR Apache-2.0 | Copyright (c) 2018 Sam Rijs, Alex Crichton and contributors |
| crossbeam-channel | 0.5.17 | MIT OR Apache-2.0 | Copyright (c) 2019 The Crossbeam Project Developers; COPYRIGHT AND/OR OTHER APPLICABLE LAW. ANY USE OF THE WORK OTHER THAN AS; Copyright (c) 2009 The Go Authors. All rights reserved. |
| crossbeam-deque | 0.8.8 | MIT OR Apache-2.0 | Copyright (c) 2019 The Crossbeam Project Developers |
| crossbeam-epoch | 0.9.21 | MIT OR Apache-2.0 | Copyright (c) 2019 The Crossbeam Project Developers |
| crossbeam-utils | 0.8.23 | MIT OR Apache-2.0 | Copyright (c) 2019 The Crossbeam Project Developers |
| crypto-common | 0.1.7 | MIT OR Apache-2.0 | Copyright (c) 2021 RustCrypto Developers |
| cssparser | 0.29.6 | MPL-2.0 | Copyright (c) Simon Sapin |
| data-encoding | 2.11.1 | MIT | Copyright (c) 2015-2020 Julien Cretin; Copyright (c) 2017-2020 Google Inc. |
| deranged | 0.5.8 | MIT OR Apache-2.0 | Copyright 2024 Jacob Pratt et al.; Copyright (c) 2024 Jacob Pratt et al. |
| digest | 0.10.7 | MIT OR Apache-2.0 | Copyright (c) 2017 Artyom Pavlov |
| dioxus | 0.7.10 | MIT OR Apache-2.0 | Copyright (c) Jonathan Kelley, Dioxus Labs, ealmloff |
| dioxus-asset-resolver | 0.7.10 | MIT OR Apache-2.0 | Copyright (c) Jonathan Kelley, Dioxus Labs |
| dioxus-cli-config | 0.7.10 | MIT OR Apache-2.0 | Copyright (c) Jonathan Kelley, Evan Almloff |
| dioxus-config-macros | 0.7.10 | MIT OR Apache-2.0 | Copyright (c) Dioxus Labs |
| dioxus-core | 0.7.10 | MIT OR Apache-2.0 | Copyright (c) Jonathan Kelley |
| dioxus-core-types | 0.7.10 | MIT OR Apache-2.0 | Copyright (c) Jonathan Kelley, Evan Almloff |
| dioxus-desktop | 0.7.10 | MIT OR Apache-2.0 | Copyright (c) Jonathan Kelley |
| dioxus-devtools | 0.7.10 | MIT OR Apache-2.0 | Copyright (c) Jonathan Kelley, Evan Almloff |
| dioxus-devtools-types | 0.7.10 | MIT OR Apache-2.0 | Copyright (c) Jonathan Kelley, Evan Almloff |
| dioxus-document | 0.7.10 | MIT OR Apache-2.0 | Copyright (c) Jonathan Kelley, Evan Almloff |
| dioxus-free-icons | 0.10.0 | MIT | Copyright (c) 2022-Present Daiki Nishikawa |
| dioxus-history | 0.7.10 | MIT OR Apache-2.0 | Copyright (c) Jonathan Kelley |
| dioxus-hooks | 0.7.10 | MIT OR Apache-2.0 | Copyright (c) Jonathan Kelley |
| dioxus-html | 0.7.10 | MIT OR Apache-2.0 | Copyright (c) Jonathan Kelley |
| dioxus-interpreter-js | 0.7.10 | MIT OR Apache-2.0 | Copyright (c) Jonathan Kelley |
| dioxus-logger | 0.7.10 | MIT | Copyright (c) DogeDark, Jonathan Kelley |
| dioxus-router | 0.7.10 | MIT OR Apache-2.0 | Copyright (c) Jonathan Kelley, Evan Almloff |
| dioxus-signals | 0.7.10 | MIT OR Apache-2.0 | Copyright (c) Jonathan Kelley, Evan Almloff |
| dioxus-stores | 0.7.10 | MIT OR Apache-2.0 | Copyright (c) Jonathan Kelley, Evan Almloff |
| dioxus-web | 0.7.10 | MIT OR Apache-2.0 | Copyright (c) Jonathan Kelley |
| dpi | 0.1.2 | Apache-2.0 AND MIT | Copyright (c) 2018 Jorge Aparicio; Copyright © 2005-2020 Rich Felker, et al.; Copyright © 1993,2004 Sun Microsystems or |
| dtoa | 1.0.11 | MIT OR Apache-2.0 | Copyright (c) David Tolnay |
| dtoa-short | 0.3.5 | MPL-2.0 | Copyright (c) Xidorn Quan |
| dunce | 1.0.5 | CC0-1.0 OR MIT-0 OR Apache-2.0 | Copyright (c) Kornel |
| either | 1.18.0 | MIT OR Apache-2.0 | Copyright (c) 2015 |
| enumset | 1.1.14 | MIT/Apache-2.0 | Copyright (c) 2017-2025 Alissa Rao <aura@aura.moe> |
| equator | 0.4.2 | MIT | Copyright (c) 2023 sarah |
| equivalent | 1.0.2 | Apache-2.0 OR MIT | Copyright (c) 2016--2023 |
| errno | 0.3.14 | MIT OR Apache-2.0 | Copyright (c) 2014 Chris Wong |
| euclid | 0.22.14 | MIT OR Apache-2.0 | Copyright (c) 2012-2013 Mozilla Foundation |
| exr | 1.74.2 | BSD-3-Clause | Copyright (c) Contributors to the OpenEXR Project. All rights reserved.; Copyright (c) Contributors to the exrs Project. All rights reserved. |
| fallible-iterator | 0.3.0 | MIT/Apache-2.0 | Copyright (c) 2015 The rust-openssl-verify Developers |
| fallible-streaming-iterator | 0.1.9 | MIT/Apache-2.0 | Copyright (c) 2016 The fallible-streaming-iterator Developers |
| fax | 0.2.7 | MIT | Copyright © 2021 The pdf-rs contributers. |
| fdeflate | 0.3.7 | MIT OR Apache-2.0 | Copyright (c) The image-rs Developers |
| flate2 | 1.1.10 | MIT OR Apache-2.0 | Copyright (c) 2014-2026 Alex Crichton |
| fnv | 1.0.7 | Apache-2.0 / MIT | Copyright (c) 2017 Contributors |
| foldhash | 0.2.0 | Zlib | Copyright (c) 2024 Orson Peters |
| foreign-types | 0.3.2 | MIT/Apache-2.0 | Copyright (c) 2017 The foreign-types Developers |
| foreign-types-shared | 0.1.1 | MIT/Apache-2.0 | Copyright (c) 2017 The foreign-types Developers |
| form_urlencoded | 1.2.2 | MIT OR Apache-2.0 | Copyright (c) 2013-2016 The rust-url developers |
| futf | 0.1.5 | MIT / Apache-2.0 | Copyright (c) 2015 Keegan McAllister |
| futures-channel | 0.3.34 | MIT OR Apache-2.0 | Copyright (c) 2016 Alex Crichton; Copyright (c) 2017 The Tokio Authors |
| futures-core | 0.3.34 | MIT OR Apache-2.0 | Copyright (c) 2016 Alex Crichton; Copyright (c) 2017 The Tokio Authors |
| futures-io | 0.3.34 | MIT OR Apache-2.0 | Copyright (c) 2016 Alex Crichton; Copyright (c) 2017 The Tokio Authors |
| futures-sink | 0.3.34 | MIT OR Apache-2.0 | Copyright (c) 2016 Alex Crichton; Copyright (c) 2017 The Tokio Authors |
| futures-task | 0.3.34 | MIT OR Apache-2.0 | Copyright (c) 2016 Alex Crichton; Copyright (c) 2017 The Tokio Authors |
| futures-util | 0.3.34 | MIT OR Apache-2.0 | Copyright (c) 2016 Alex Crichton; Copyright (c) 2017 The Tokio Authors |
| fxhash | 0.2.1 | Apache-2.0/MIT | Copyright (c) cbreeden |
| generational-box | 0.7.10 | MIT OR Apache-2.0 | Copyright (c) Evan Almloff |
| generic-array | 0.14.7 | MIT | Copyright (c) 2015 Bartłomiej Kamiński |
| getrandom | 0.2.17 | MIT OR Apache-2.0 | Copyright (c) 2018-2024 The rust-random Project Developers; Copyright (c) 2014 The Rust Project Developers |
| getrandom | 0.3.4 | MIT OR Apache-2.0 | Copyright (c) 2018-2025 The rust-random Project Developers; Copyright (c) 2014 The Rust Project Developers |
| getrandom | 0.4.3 | MIT OR Apache-2.0 | Copyright (c) 2018-2026 The rust-random Project Developers; Copyright (c) 2014 The Rust Project Developers |
| gif | 0.14.2 | MIT OR Apache-2.0 | Copyright (c) 2015 nwin |
| globset | 0.4.20 | Unlicense OR MIT | Copyright (c) 2015 Andrew Gallant |
| globwalk | 0.8.1 | MIT | Copyright (c) 2017 Gilad Naaman |
| gloo-timers | 0.3.0 | MIT OR Apache-2.0 | Copyright (c) Rust and WebAssembly Working Group |
| half | 2.7.1 | MIT OR Apache-2.0 | Copyright (c) Kathryn Long |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | Copyright (c) 2016 Amanieu d'Antras |
| hashlink | 0.12.2 | MIT OR Apache-2.0 | Copyright (c) the contributors of https://github.com/djc/hashlink |
| html5ever | 0.29.1 | MIT OR Apache-2.0 | Copyright (c) 2014 The html5ever Project Developers |
| http | 1.5.0 | MIT OR Apache-2.0 | Copyright 2017 http-rs authors; Copyright (c) 2017 http-rs authors |
| httparse | 1.10.1 | MIT OR Apache-2.0 | Copyright (c) 2015-2025 Sean McArthur |
| icu_collections | 2.3.0 | Unicode-3.0 | COPYRIGHT AND PERMISSION NOTICE; Copyright © 2020-2024 Unicode, Inc. |
| icu_locale_core | 2.3.0 | Unicode-3.0 | COPYRIGHT AND PERMISSION NOTICE; Copyright © 2020-2024 Unicode, Inc. |
| icu_normalizer | 2.3.0 | Unicode-3.0 | COPYRIGHT AND PERMISSION NOTICE; Copyright © 2020-2024 Unicode, Inc. |
| icu_normalizer_data | 2.3.0 | Unicode-3.0 | COPYRIGHT AND PERMISSION NOTICE; Copyright © 2020-2024 Unicode, Inc. |
| icu_properties | 2.3.0 | Unicode-3.0 | COPYRIGHT AND PERMISSION NOTICE; Copyright © 2020-2024 Unicode, Inc. |
| icu_properties_data | 2.3.0 | Unicode-3.0 | COPYRIGHT AND PERMISSION NOTICE; Copyright © 2020-2024 Unicode, Inc. |
| icu_provider | 2.3.1 | Unicode-3.0 | COPYRIGHT AND PERMISSION NOTICE; Copyright © 2020-2024 Unicode, Inc. |
| idna | 1.1.0 | MIT OR Apache-2.0 | Copyright (c) 2013-2025 The rust-url developers |
| idna_adapter | 1.2.2 | Apache-2.0 OR MIT | Copyright (c) The rust-url developers |
| ignore | 0.4.33 | Unlicense OR MIT | Copyright (c) 2015 Andrew Gallant |
| image | 0.25.10 | MIT OR Apache-2.0 | Copyright (c) The image-rs Developers |
| image-webp | 0.2.4 | MIT OR Apache-2.0 | Copyright (c) the contributors of https://github.com/image-rs/image-webp |
| imgref | 1.12.3 | CC0-1.0 OR Apache-2.0 | Copyright (c) Kornel Lesiński |
| indexmap | 2.14.2 | Apache-2.0 OR MIT | Copyright (c) 2016--2017 |
| infer | 0.19.0 | MIT | Copyright (c) 2019 Bojan |
| iso_country | 0.1.4 | MIT | Copyright (c) 2016 Piotr Zolnierek |
| iso_currency | 0.7.1 | MIT | Copyright (c) 2019 Rostislav Raykov <z@zbrox.org> |
| itertools | 0.11.0 | MIT OR Apache-2.0 | Copyright (c) 2015 |
| itertools | 0.14.0 | MIT OR Apache-2.0 | Copyright (c) 2015 |
| itoa | 1.0.18 | MIT OR Apache-2.0 | Copyright (c) David Tolnay |
| jni | 0.21.1 | MIT/Apache-2.0 | Copyright (c) 2016 Prevoty, Inc. and jni-rs contributors |
| jni | 0.22.4 | MIT OR Apache-2.0 | Copyright (c) jni team |
| jni-sys | 0.3.1 | MIT OR Apache-2.0 | Copyright (c) 2015 The rust-jni-sys Developers |
| jni-sys | 0.4.1 | MIT OR Apache-2.0 | Copyright (c) 2015 The rust-jni-sys Developers |
| js-sys | 0.3.106 | MIT OR Apache-2.0 | Copyright (c) 2014 Alex Crichton |
| keyboard-types | 0.7.0 | MIT OR Apache-2.0 | Copyright (c) 2017 Pyfisch |
| konst | 0.2.20 | Zlib | Copyright (c) 2021 Matias Rodriguez. |
| konst_macro_rules | 0.2.19 | Zlib | Copyright (c) 2021 Matias Rodriguez. |
| kuchikiki | 0.8.8-speedreader | MIT | Copyright (c) Brave Authors, Ralph Giles, Simon Sapin |
| lazy_static | 1.5.1 | MIT OR Apache-2.0 | Copyright (c) Marvin Löbel |
| lebe | 0.5.3 | BSD-3-Clause | Copyright (c) 2022 Contributors to the lebe Project. All rights reserved. |
| libc | 0.2.190 | MIT OR Apache-2.0 | Copyright (c) The Rust Project Developers |
| libloading | 0.8.9 | ISC | Copyright © 2015, Simonas Kazlauskas |
| libm | 0.2.16 | MIT | Copyright (c) 2018 Jorge Aparicio; Copyright © 2005-2020 Rich Felker, et al.; Copyright © 1993,2004 Sun Microsystems or |
| libsqlite3-sys | 0.38.2 | MIT | Copyright (c) 2014 The rusqlite developers |
| linux-raw-sys | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Copyright (c) Dan Gohman |
| litemap | 0.8.3 | Unicode-3.0 | COPYRIGHT AND PERMISSION NOTICE; Copyright © 2020-2024 Unicode, Inc. |
| lock_api | 0.4.14 | MIT OR Apache-2.0 | Copyright (c) 2016 The Rust Project Developers |
| log | 0.4.34 | MIT OR Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| longest-increasing-subsequence | 0.1.0 | MIT/Apache-2.0 | Copyright (c) 2014 Alex Crichton |
| loop9 | 0.1.5 | MIT | Copyright (c) Kornel |
| mac | 0.1.1 | MIT/Apache-2.0 | Copyright (c) Jonathan Reem |
| manganis | 0.7.10 | MIT OR Apache-2.0 | Copyright (c) Evan Almloff |
| manganis-core | 0.7.10 | MIT OR Apache-2.0 | Copyright (c) Jonathan Kelley |
| markup5ever | 0.14.1 | MIT OR Apache-2.0 | Copyright (c) 2014 The html5ever Project Developers |
| matchers | 0.2.0 | MIT | Copyright (c) 2019 Eliza Weisman |
| matches | 0.1.10 | MIT | Copyright (c) 2014-2016 Simon Sapin |
| maybe-rayon | 0.1.1 | MIT | Copyright (c) 2021 Joshua Holmer |
| memchr | 2.8.3 | Unlicense OR MIT | Copyright (c) 2015 Andrew Gallant |
| memfd | 0.6.6 | MIT OR Apache-2.0 | Copyright (c) Luca Bruno, Simonas Kazlauskas |
| memmap2 | 0.9.11 | MIT OR Apache-2.0 | Copyright [2015] [Dan Burkert]; Copyright (c) 2020 Yevhenii Reizner; Copyright (c) 2015 Dan Burkert |
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 | Copyright 2013-2014 RAD Game Tools and Valve Software; Copyright 2010-2014 Rich Geldreich and Tenacious Software LLC; Copyright (c) 2017 Frommi |
| miniz_oxide | 0.9.1 | MIT OR Zlib OR Apache-2.0 | Copyright 2013-2014 RAD Game Tools and Valve Software; Copyright 2010-2014 Rich Geldreich and Tenacious Software LLC; Copyright (c) 2017 Frommi |
| moxcms | 0.8.1 | BSD-3-Clause OR Apache-2.0 | Copyright 2024 Radzivon Bartoshyk; Copyright (c) Radzivon Bartoshyk. All rights reserved. |
| native-tls | 0.2.18 | MIT OR Apache-2.0 | Copyright (c) 2016 The rust-native-tls Developers |
| ndk | 0.9.0 | MIT OR Apache-2.0 | Copyright (c) The Rust Mobile contributors |
| ndk-context | 0.1.1 | MIT OR Apache-2.0 | Copyright (c) The Rust Windowing contributors |
| ndk-sys | 0.6.0+11769913 | MIT OR Apache-2.0 | Copyright (c) The Rust Windowing contributors |
| new_debug_unreachable | 1.0.6 | MIT | Copyright (c) 2015 Jonathan Reem |
| no_std_io2 | 0.9.4 | Apache-2.0 OR MIT | Copyright (c) 2020-2021  Brendan Molloy <brendan@bbqsrc.net> |
| nodrop | 0.1.14 | MIT/Apache-2.0 | Copyright (c) Ulrik Sverdrup "bluss" 2015-2017 |
| nom | 8.0.0 | MIT | Copyright (c) 2014-2019 Geoffroy Couprie |
| normpath | 1.5.2 | MIT OR Apache-2.0 | Copyright (c) 2020 dylni (https://github.com/dylni); Copyright (c) 2020 Nikolai Vazquez |
| num-bigint | 0.4.8 | MIT OR Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| num-complex | 0.4.6 | MIT OR Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| num-conv | 0.2.2 | MIT OR Apache-2.0 | Copyright (c) Jacob Pratt |
| num-integer | 0.1.47 | MIT OR Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| num-rational | 0.4.2 | MIT OR Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| num-traits | 0.2.19 | MIT OR Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| num_cpus | 1.17.0 | MIT OR Apache-2.0 | Copyright (c) 2015-2025 Sean McArthur |
| num_enum | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | Copyright (c) 2018, Daniel Wagner-Hall |
| num_threads | 0.1.7 | MIT OR Apache-2.0 | Copyright 2021 Jacob Pratt; Copyright (c) 2021 Jacob Pratt |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | Copyright (c) Aleksey Kladov |
| openssl | 0.10.81 | Apache-2.0 | Copyright 2011-2017 Google Inc. |
| openssl-probe | 0.2.1 | MIT OR Apache-2.0 | Copyright (c) 2014 Alex Crichton |
| openssl-sys | 0.9.117 | MIT | Copyright (c) 2014 Alex Crichton |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 | Copyright (c) 2016 The Rust Project Developers |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 | Copyright (c) 2016 The Rust Project Developers |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 | Copyright (c) 2013-2025 The rust-url developers |
| phf | 0.10.1 | MIT | Copyright (c) Steven Fackler |
| phf | 0.11.3 | MIT | Copyright (c) 2014-2022 Steven Fackler, Yuki Okushi |
| phf | 0.8.0 | MIT | Copyright (c) Steven Fackler |
| phf_shared | 0.10.0 | MIT | Copyright (c) Steven Fackler |
| phf_shared | 0.11.3 | MIT | Copyright (c) 2014-2022 Steven Fackler, Yuki Okushi |
| phf_shared | 0.8.0 | MIT | Copyright (c) Steven Fackler |
| pin-project | 1.1.13 | Apache-2.0 OR MIT | Copyright (c) the contributors of https://github.com/taiki-e/pin-project |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | Copyright (c) the contributors of https://github.com/taiki-e/pin-project-lite |
| png | 0.18.1 | MIT OR Apache-2.0 | Copyright (c) 2015 nwin |
| potential_utf | 0.1.6 | Unicode-3.0 | COPYRIGHT AND PERMISSION NOTICE; Copyright © 2020-2024 Unicode, Inc. |
| powerfmt | 0.2.0 | MIT OR Apache-2.0 | Copyright 2023 Jacob Pratt et al.; Copyright (c) 2023 Jacob Pratt et al. |
| ppv-lite86 | 0.2.21 | MIT OR Apache-2.0 | Copyright 2019 The CryptoCorrosion Contributors; Copyright (c) 2019 The CryptoCorrosion Contributors |
| precomputed-hash | 0.1.1 | MIT | Copyright (c) 2017 Emilio Cobos Álvarez |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 | Copyright (c) David Tolnay, Alex Crichton |
| profiling | 1.0.18 | MIT OR Apache-2.0 | Copyright (c) Philip Degarmo |
| pulp | 0.22.3 | MIT | Copyright (c) 2021 sarah |
| pulp-wasm-simd-flag | 0.1.1 | MIT | Copyright (c) sarah quiñones |
| pxfm | 0.1.30 | BSD-3-Clause OR Apache-2.0 | Copyright 2024 Radzivon Bartoshyk; Copyright (c) Radzivon Bartoshyk. All rights reserved. |
| qoi | 0.4.1 | MIT/Apache-2.0 | Copyright (c) 2022 Ivan Smirnov |
| quick-error | 2.0.1 | MIT/Apache-2.0 | Copyright (c) 2015 The quick-error Developers |
| rand | 0.8.8 | MIT OR Apache-2.0 | Copyright 2018 Developers of the Rand project; Copyright (c) 2014 The Rust Project Developers |
| rand | 0.9.5 | MIT OR Apache-2.0 | Copyright 2018 Developers of the Rand project; Copyright (c) 2014 The Rust Project Developers |
| rand_chacha | 0.3.1 | MIT OR Apache-2.0 | Copyright 2018 Developers of the Rand project; Copyright (c) 2014 The Rust Project Developers |
| rand_chacha | 0.9.0 | MIT OR Apache-2.0 | Copyright 2018 Developers of the Rand project; Copyright (c) 2014 The Rust Project Developers |
| rand_core | 0.6.4 | MIT OR Apache-2.0 | Copyright 2018 Developers of the Rand project; Copyright (c) 2014 The Rust Project Developers |
| rand_core | 0.9.5 | MIT OR Apache-2.0 | Copyright 2018 Developers of the Rand project; Copyright (c) 2014 The Rust Project Developers |
| rav1e | 0.8.1 | BSD-2-Clause | Copyright (c) 2017-2023, the rav1e contributors |
| ravif | 0.13.0 | BSD-3-Clause | Copyright (c) 2020, Kornel |
| raw-window-handle | 0.5.2 | MIT OR Apache-2.0 OR Zlib | Copyright (c) 2019 Osspial; Copyright (c) 2020 Osspial |
| raw-window-handle | 0.6.2 | MIT OR Apache-2.0 OR Zlib | Copyright (c) 2019 Osspial; Copyright (c) 2020 Osspial |
| rayon | 1.12.0 | MIT OR Apache-2.0 | Copyright (c) 2010 The Rust Project Developers |
| rayon-core | 1.13.0 | MIT OR Apache-2.0 | Copyright (c) 2010 The Rust Project Developers |
| reborrow | 0.5.5 | MIT | Copyright (c) 2022 sarah |
| regex | 1.13.1 | MIT OR Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| regex-automata | 0.4.18 | MIT OR Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| rgb | 0.8.53 | MIT | Copyright (c) 2019 Kornel |
| ring | 0.17.14 | Apache-2.0 AND ISC | Copyright (c) 2009 The Go Authors. All rights reserved.; Copyright 2015 The Chromium Authors. All rights reserved.; Copyright 2015-2025 Brian Smith. |
| rten | 0.27.0 | MIT OR Apache-2.0 | Copyright (c) Robert Knight |
| rten-base | 0.27.0 | MIT OR Apache-2.0 | Copyright (c) Robert Knight |
| rten-gemm | 0.27.0 | MIT OR Apache-2.0 | Copyright (c) Robert Knight |
| rten-imageproc | 0.27.0 | MIT OR Apache-2.0 | Copyright (c) Robert Knight |
| rten-onnx | 0.27.0 | MIT OR Apache-2.0 | Copyright (c) Robert Knight |
| rten-parallel | 0.27.0 | MIT OR Apache-2.0 | Copyright (c) Robert Knight |
| rten-shape-inference | 0.27.0 | MIT OR Apache-2.0 | Copyright (c) Robert Knight |
| rten-simd | 0.27.0 | MIT OR Apache-2.0 | Copyright (c) Robert Knight |
| rten-tensor | 0.27.0 | MIT OR Apache-2.0 | Copyright (c) Robert Knight |
| rten-vecmath | 0.27.0 | MIT OR Apache-2.0 | Copyright (c) Robert Knight |
| rusqlite | 0.40.2 | MIT | Copyright (c) 2014 The rusqlite developers |
| rust-i18n | 3.1.5 | MIT | Copyright (c) 2021 Longbridge |
| rust-i18n-support | 3.1.5 | MIT | Copyright (c) the contributors of https://github.com/longbridge/rust-i18n |
| rust_decimal | 1.43.0 | MIT | Copyright (c) 2016 Paul Mason |
| rustc-hash | 1.1.0 | Apache-2.0/MIT | Copyright (c) The Rust Project Developers |
| rustc-hash | 2.1.3 | Apache-2.0 OR MIT | Copyright (c) The Rust Project Developers |
| rustix | 1.1.5 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Copyright (c) Dan Gohman, Jakub Konka |
| rustls | 0.23.45 | Apache-2.0 OR ISC OR MIT | Copyright (c) 2016, Joseph Birr-Pixton <jpixton@gmail.com>; Copyright (c) 2016 Joseph Birr-Pixton <jpixton@gmail.com> |
| rustls-pki-types | 1.15.1 | MIT OR Apache-2.0 | Copyright 2023 Dirkjan Ochtman; Copyright (c) 2023 Dirkjan Ochtman <dirkjan@ochtman.nl> |
| rustls-webpki | 0.103.15 | ISC | Copyright 2015 Brian Smith. |
| ryu | 1.0.23 | Apache-2.0 OR BSL-1.0 | Copyright (c) David Tolnay |
| same-file | 1.0.6 | Unlicense/MIT | Copyright (c) 2017 Andrew Gallant |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 | Copyright (c) 2016-2019 Ulrik Sverdrup "bluss" and scopeguard developers |
| selectors | 0.24.0 | MPL-2.0 | Copyright (c) The Servo Project Developers |
| send_wrapper | 0.6.0 | MIT/Apache-2.0 | Copyright (c) Thomas Keh |
| serde | 1.0.229 | MIT OR Apache-2.0 | Copyright (c) Erick Tryzelaar, David Tolnay |
| serde-wasm-bindgen | 0.6.5 | MIT | Copyright (c) 2019 Cloudflare, Inc. |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | Copyright (c) Erick Tryzelaar, David Tolnay |
| serde_json | 1.0.151 | MIT OR Apache-2.0 | Copyright (c) Erick Tryzelaar, David Tolnay |
| serde_spanned | 0.6.9 | MIT OR Apache-2.0 | Copyright (c) Individual contributors |
| serde_yaml | 0.9.34+deprecated | MIT OR Apache-2.0 | Copyright (c) David Tolnay |
| servo_arc | 0.2.0 | MIT OR Apache-2.0 | Copyright (c) The Servo Project Developers |
| sha1 | 0.10.7 | MIT OR Apache-2.0 | Copyright (c) 2006-2009 Graydon Hoare; Copyright (c) 2009-2013 Mozilla Foundation; Copyright (c) 2016 Artyom Pavlov |
| sha2 | 0.10.9 | MIT OR Apache-2.0 | Copyright (c) 2006-2009 Graydon Hoare; Copyright (c) 2009-2013 Mozilla Foundation; Copyright (c) 2016 Artyom Pavlov |
| sharded-slab | 0.1.7 | MIT | Copyright (c) 2019 Eliza Weisman |
| signal-hook | 0.3.18 | Apache-2.0/MIT | Copyright (c) 2017 tokio-jsonrpc developers |
| signal-hook-registry | 1.4.8 | MIT OR Apache-2.0 | Copyright (c) 2017 tokio-jsonrpc developers |
| simd-adler32 | 0.3.10 | MIT | Copyright (c) [2021] [Marvin Countryman] |
| simd_cesu8 | 1.2.0 | Apache-2.0 OR MIT | Copyright (c) Sean C. Roach |
| simdutf8 | 0.1.5 | MIT OR Apache-2.0 | Copyright (c) Hans Kratz |
| siphasher | 0.3.11 | MIT/Apache-2.0 | Copyright 2012-2016 The Rust Project Developers.; Copyright 2016-2023 Frank Denis. |
| siphasher | 1.0.4 | MIT OR Apache-2.0 | Copyright 2012-2016 The Rust Project Developers.; Copyright 2016-2026 Frank Denis. |
| slab | 0.4.12 | MIT | Copyright (c) 2019 Carl Lerche |
| sledgehammer_bindgen | 0.6.0 | MIT | Copyright (c) Evan Almloff |
| sledgehammer_utils | 0.3.1 | MIT | Copyright (c) Evan Almloff |
| slotmap | 1.1.1 | Zlib | Copyright (c) 2021 Orson Peters <orsonpeters@gmail.com> |
| smallvec | 1.16.2 | MIT OR Apache-2.0 | Copyright (c) 2018 The Servo Project Developers |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 | Copyright (c) 2017 Robert Grosse |
| string_cache | 0.8.9 | MIT OR Apache-2.0 | Copyright (c) 2012-2013 Mozilla Foundation |
| strum | 0.27.2 | MIT | Copyright (c) 2019 Peter Glotfelty |
| subsecond | 0.7.10 | MIT OR Apache-2.0 | Copyright (c) Jonathan Kelley |
| subsecond-types | 0.7.10 | MIT OR Apache-2.0 | Copyright (c) Jonathan Kelley |
| subtle | 2.6.1 | BSD-3-Clause | Copyright (c) 2016-2017 Isis Agora Lovecruft, Henry de Valence. All rights reserved.; Copyright (c) 2016-2024 Isis Agora Lovecruft. All rights reserved. |
| sys-locale | 0.3.2 | MIT OR Apache-2.0 | Copyright (c) 2021 1Password |
| tao | 0.34.8 | Apache-2.0 | Copyright (c) Tauri Programme within The Commons Conservancy, The winit contributors |
| tendril | 0.4.3 | MIT/Apache-2.0 | Copyright (c) 2015 Keegan McAllister |
| thiserror | 1.0.69 | MIT OR Apache-2.0 | Copyright (c) David Tolnay |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | Copyright (c) David Tolnay |
| thread_local | 1.1.10 | MIT OR Apache-2.0 | Copyright (c) 2016 The Rust Project Developers |
| tiff | 0.11.3 | MIT | Copyright (c) 2018 PistonDevelopers |
| time | 0.3.55 | MIT OR Apache-2.0 | Copyright (c) Jacob Pratt et al. |
| time-core | 0.1.9 | MIT OR Apache-2.0 | Copyright (c) Jacob Pratt et al. |
| tinystr | 0.8.4 | Unicode-3.0 | COPYRIGHT AND PERMISSION NOTICE; Copyright © 2020-2024 Unicode, Inc. |
| tokio | 1.53.2 | MIT | Copyright (c) Tokio Contributors |
| toml | 0.8.23 | MIT OR Apache-2.0 | Copyright (c) Individual contributors |
| toml_datetime | 0.6.11 | MIT OR Apache-2.0 | Copyright (c) Individual contributors |
| toml_edit | 0.22.27 | MIT OR Apache-2.0 | Copyright (c) Individual contributors |
| toml_write | 0.1.2 | MIT OR Apache-2.0 | Copyright (c) Individual contributors |
| tracing | 0.1.44 | MIT | Copyright (c) 2019 Tokio Contributors |
| tracing-core | 0.1.36 | MIT | Copyright (c) 2019 Tokio Contributors |
| tracing-subscriber | 0.3.23 | MIT | Copyright (c) 2019 Tokio Contributors |
| triomphe | 0.1.16 | MIT OR Apache-2.0 | Copyright (c) 2019 Manish Goregaokar |
| tungstenite | 0.28.0 | MIT OR Apache-2.0 | Copyright (c) 2017 Alexey Galakhov; Copyright (c) 2016 Jason Housley |
| typeid | 1.0.3 | MIT OR Apache-2.0 | Copyright (c) David Tolnay |
| typenum | 1.20.1 | MIT OR Apache-2.0 | Copyright 2014 Paho Lurie-Gregg; Copyright (c) 2014 Paho Lurie-Gregg |
| unicode-ident | 1.0.26 | (MIT OR Apache-2.0) AND Unicode-3.0 | COPYRIGHT AND PERMISSION NOTICE; Copyright © 1991-2023 Unicode, Inc. |
| unicode-segmentation | 1.13.3 | MIT OR Apache-2.0 | Copyright (c) 2015 The Rust Project Developers |
| unsafe-libyaml | 0.2.11 | MIT | Copyright (c) David Tolnay |
| untrusted | 0.9.0 | ISC | Copyright 2015-2016 Brian Smith. |
| ureq | 3.4.2 | MIT OR Apache-2.0 | Copyright (c) 2019 Martin Algesten |
| ureq-proto | 0.6.4 | MIT OR Apache-2.0 | Copyright 2022 Martin Algesten |
| url | 2.5.8 | MIT OR Apache-2.0 | Copyright (c) 2013-2025 The rust-url developers |
| utf-8 | 0.7.6 | MIT OR Apache-2.0 | Copyright (c) Simon Sapin |
| utf8-zero | 0.8.1 | MIT OR Apache-2.0 | Copyright (c) Simon Sapin, Martin Algesten |
| utf8_iter | 1.0.4 | Apache-2.0 OR MIT | Copyright Mozilla Foundation |
| uuid | 1.27.0 | Apache-2.0 OR MIT | Copyright (c) 2014 The Rust Project Developers; Copyright (c) 2018 Ashley Mannix, Christopher Armstrong, Dylan DPC, Hunar Roop Kahlon |
| v_frame | 0.3.9 | BSD-2-Clause | Copyright (c) 2017-2022, the rav1e contributors |
| walkdir | 2.5.0 | Unlicense/MIT | Copyright (c) 2015 Andrew Gallant |
| warnings | 0.2.1 | MIT OR Apache-2.0 | Copyright (c) Evan Almloff |
| wasm-bindgen | 0.2.129 | MIT OR Apache-2.0 | Copyright (c) 2014 Alex Crichton |
| wasm-bindgen-futures | 0.4.79 | MIT OR Apache-2.0 | Copyright (c) 2014 Alex Crichton |
| wasm-bindgen-shared | 0.2.129 | MIT OR Apache-2.0 | Copyright (c) 2014 Alex Crichton |
| wasm-streams | 0.4.2 | MIT OR Apache-2.0 | Copyright (c) Mattias Buelens |
| web-sys | 0.3.106 | MIT OR Apache-2.0 | Copyright (c) 2014 Alex Crichton |
| webbrowser | 1.2.4 | MIT OR Apache-2.0 | Copyright (c) 2015-2022 Amod Malviya |
| webpki-roots | 1.0.9 | CDLA-Permissive-2.0 | Copyright (c) the contributors of https://github.com/rustls/webpki-roots |
| weezl | 0.1.12 | MIT OR Apache-2.0 | Copyright (c) HeroicKatora 2020 |
| winnow | 0.7.15 | MIT | Copyright (c) the contributors of https://github.com/winnow-rs/winnow |
| writeable | 0.6.4 | Unicode-3.0 | COPYRIGHT AND PERMISSION NOTICE; Copyright © 2020-2024 Unicode, Inc. |
| wry | 0.53.5 | Apache-2.0 OR MIT | Copyright (c) 2020-2023 Ngo Iok Ui & Tauri Programme within The Commons Conservancy |
| y4m | 0.8.0 | MIT | Copyright (c) 2015-2019 PistonDevelopers; Copyright (c) 2019 image-rs contributors |
| yoke | 0.8.3 | Unicode-3.0 | COPYRIGHT AND PERMISSION NOTICE; Copyright © 2020-2024 Unicode, Inc. |
| zerocopy | 0.8.59 | BSD-2-Clause OR Apache-2.0 OR MIT | Copyright 2023 The Fuchsia Authors; Copyright 2019 The Fuchsia Authors. |
| zerofrom | 0.1.8 | Unicode-3.0 | COPYRIGHT AND PERMISSION NOTICE; Copyright © 2020-2024 Unicode, Inc. |
| zeroize | 1.9.0 | Apache-2.0 OR MIT | Copyright (c) 2018-2026 The RustCrypto Project Developers |
| zerotrie | 0.2.5 | Unicode-3.0 | COPYRIGHT AND PERMISSION NOTICE; Copyright © 2020-2024 Unicode, Inc. |
| zerovec | 0.11.8 | Unicode-3.0 | COPYRIGHT AND PERMISSION NOTICE; Copyright © 2020-2024 Unicode, Inc. |
| zlib-rs | 0.6.8 | Zlib | Copyright (c) the contributors of https://github.com/trifectatechfoundation/zlib-rs |
| zmij | 1.0.23 | MIT | Copyright (c) David Tolnay |
| zune-core | 0.5.3 | MIT OR Apache-2.0 OR Zlib | Copyright (c) zune-image developers |
| zune-inflate | 0.2.54 | MIT OR Apache-2.0 OR Zlib | – |
| zune-jpeg | 0.5.15 | MIT OR Apache-2.0 OR Zlib | Copyright (c) zune-image developers |

## Lizenztexte

### MIT

```text
Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
```

### Apache-2.0

```text
Apache License
                        Version 2.0, January 2004
                     http://www.apache.org/licenses/

TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

1. Definitions.

   "License" shall mean the terms and conditions for use, reproduction,
   and distribution as defined by Sections 1 through 9 of this document.

   "Licensor" shall mean the copyright owner or entity authorized by
   the copyright owner that is granting the License.

   "Legal Entity" shall mean the union of the acting entity and all
   other entities that control, are controlled by, or are under common
   control with that entity. For the purposes of this definition,
   "control" means (i) the power, direct or indirect, to cause the
   direction or management of such entity, whether by contract or
   otherwise, or (ii) ownership of fifty percent (50%) or more of the
   outstanding shares, or (iii) beneficial ownership of such entity.

   "You" (or "Your") shall mean an individual or Legal Entity
   exercising permissions granted by this License.

   "Source" form shall mean the preferred form for making modifications,
   including but not limited to software source code, documentation
   source, and configuration files.

   "Object" form shall mean any form resulting from mechanical
   transformation or translation of a Source form, including but
   not limited to compiled object code, generated documentation,
   and conversions to other media types.

   "Work" shall mean the work of authorship, whether in Source or
   Object form, made available under the License, as indicated by a
   copyright notice that is included in or attached to the work
   (an example is provided in the Appendix below).

   "Derivative Works" shall mean any work, whether in Source or Object
   form, that is based on (or derived from) the Work and for which the
   editorial revisions, annotations, elaborations, or other modifications
   represent, as a whole, an original work of authorship. For the purposes
   of this License, Derivative Works shall not include works that remain
   separable from, or merely link (or bind by name) to the interfaces of,
   the Work and Derivative Works thereof.

   "Contribution" shall mean any work of authorship, including
   the original version of the Work and any modifications or additions
   to that Work or Derivative Works thereof, that is intentionally
   submitted to Licensor for inclusion in the Work by the copyright owner
   or by an individual or Legal Entity authorized to submit on behalf of
   the copyright owner. For the purposes of this definition, "submitted"
   means any form of electronic, verbal, or written communication sent
   to the Licensor or its representatives, including but not limited to
   communication on electronic mailing lists, source code control systems,
   and issue tracking systems that are managed by, or on behalf of, the
   Licensor for the purpose of discussing and improving the Work, but
   excluding communication that is conspicuously marked or otherwise
   designated in writing by the copyright owner as "Not a Contribution."

   "Contributor" shall mean Licensor and any individual or Legal Entity
   on behalf of whom a Contribution has been received by Licensor and
   subsequently incorporated within the Work.

2. Grant of Copyright License. Subject to the terms and conditions of
   this License, each Contributor hereby grants to You a perpetual,
   worldwide, non-exclusive, no-charge, royalty-free, irrevocable
   copyright license to reproduce, prepare Derivative Works of,
   publicly display, publicly perform, sublicense, and distribute the
   Work and such Derivative Works in Source or Object form.

3. Grant of Patent License. Subject to the terms and conditions of
   this License, each Contributor hereby grants to You a perpetual,
   worldwide, non-exclusive, no-charge, royalty-free, irrevocable
   (except as stated in this section) patent license to make, have made,
   use, offer to sell, sell, import, and otherwise transfer the Work,
   where such license applies only to those patent claims licensable
   by such Contributor that are necessarily infringed by their
   Contribution(s) alone or by combination of their Contribution(s)
   with the Work to which such Contribution(s) was submitted. If You
   institute patent litigation against any entity (including a
   cross-claim or counterclaim in a lawsuit) alleging that the Work
   or a Contribution incorporated within the Work constitutes direct
   or contributory patent infringement, then any patent licenses
   granted to You under this License for that Work shall terminate
   as of the date such litigation is filed.

4. Redistribution. You may reproduce and distribute copies of the
   Work or Derivative Works thereof in any medium, with or without
   modifications, and in Source or Object form, provided that You
   meet the following conditions:

   (a) You must give any other recipients of the Work or
       Derivative Works a copy of this License; and

   (b) You must cause any modified files to carry prominent notices
       stating that You changed the files; and

   (c) You must retain, in the Source form of any Derivative Works
       that You distribute, all copyright, patent, trademark, and
       attribution notices from the Source form of the Work,
       excluding those notices that do not pertain to any part of
       the Derivative Works; and

   (d) If the Work includes a "NOTICE" text file as part of its
       distribution, then any Derivative Works that You distribute must
       include a readable copy of the attribution notices contained
       within such NOTICE file, excluding those notices that do not
       pertain to any part of the Derivative Works, in at least one
       of the following places: within a NOTICE text file distributed
       as part of the Derivative Works; within the Source form or
       documentation, if provided along with the Derivative Works; or,
       within a display generated by the Derivative Works, if and
       wherever such third-party notices normally appear. The contents
       of the NOTICE file are for informational purposes only and
       do not modify the License. You may add Your own attribution
       notices within Derivative Works that You distribute, alongside
       or as an addendum to the NOTICE text from the Work, provided
       that such additional attribution notices cannot be construed
       as modifying the License.

   You may add Your own copyright statement to Your modifications and
   may provide additional or different license terms and conditions
   for use, reproduction, or distribution of Your modifications, or
   for any such Derivative Works as a whole, provided Your use,
   reproduction, and distribution of the Work otherwise complies with
   the conditions stated in this License.

5. Submission of Contributions. Unless You explicitly state otherwise,
   any Contribution intentionally submitted for inclusion in the Work
   by You to the Licensor shall be under the terms and conditions of
   this License, without any additional terms or conditions.
   Notwithstanding the above, nothing herein shall supersede or modify
   the terms of any separate license agreement you may have executed
   with Licensor regarding such Contributions.

6. Trademarks. This License does not grant permission to use the trade
   names, trademarks, service marks, or product names of the Licensor,
   except as required for reasonable and customary use in describing the
   origin of the Work and reproducing the content of the NOTICE file.

7. Disclaimer of Warranty. Unless required by applicable law or
   agreed to in writing, Licensor provides the Work (and each
   Contributor provides its Contributions) on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
   implied, including, without limitation, any warranties or conditions
   of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A
   PARTICULAR PURPOSE. You are solely responsible for determining the
   appropriateness of using or redistributing the Work and assume any
   risks associated with Your exercise of permissions under this License.

8. Limitation of Liability. In no event and under no legal theory,
   whether in tort (including negligence), contract, or otherwise,
   unless required by applicable law (such as deliberate and grossly
   negligent acts) or agreed to in writing, shall any Contributor be
   liable to You for damages, including any direct, indirect, special,
   incidental, or consequential damages of any character arising as a
   result of this License or out of the use or inability to use the
   Work (including but not limited to damages for loss of goodwill,
   work stoppage, computer failure or malfunction, or any and all
   other commercial damages or losses), even if such Contributor
   has been advised of the possibility of such damages.

9. Accepting Warranty or Additional Liability. While redistributing
   the Work or Derivative Works thereof, You may choose to offer,
   and charge a fee for, acceptance of support, warranty, indemnity,
   or other liability obligations and/or rights consistent with this
   License. However, in accepting such obligations, You may act only
   on Your own behalf and on Your sole responsibility, not on behalf
   of any other Contributor, and only if You agree to indemnify,
   defend, and hold each Contributor harmless for any liability
   incurred by, or claims asserted against, such Contributor by reason
   of your accepting any such warranty or additional liability.

END OF TERMS AND CONDITIONS
```

### BSD-3-Clause

```text
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

1. Redistributions of source code must retain the above copyright
notice, this list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright
notice, this list of conditions and the following disclaimer in the
documentation and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its
contributors may be used to endorse or promote products derived from
this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS
IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A
PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED
TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### BSD-2-Clause

```text
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

* Redistributions of source code must retain the above copyright notice, this
  list of conditions and the following disclaimer.

* Redistributions in binary form must reproduce the above copyright notice,
  this list of conditions and the following disclaimer in the documentation
  and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### ISC

```text
Permission to use, copy, modify, and/or distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHORS DISCLAIM ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR
// ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
// ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
// OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
```

### Zlib

```text
This software is provided 'as-is', without any express or implied warranty. In
no event will the authors be held liable for any damages arising from the use of
this software.

Permission is granted to anyone to use this software for any purpose, including
commercial applications, and to alter it and redistribute it freely, subject to
the following restrictions:

1. The origin of this software must not be misrepresented; you must not claim
    that you wrote the original software. If you use this software in a product,
    an acknowledgment in the product documentation would be appreciated but is
    not required.

2. Altered source versions must be plainly marked as such, and must not be
    misrepresented as being the original software.

3. This notice may not be removed or altered from any source distribution.
```

### Unicode-3.0

```text
UNICODE LICENSE V3

COPYRIGHT AND PERMISSION NOTICE

Copyright © 2020-2024 Unicode, Inc.

NOTICE TO USER: Carefully read the following legal agreement. BY
DOWNLOADING, INSTALLING, COPYING OR OTHERWISE USING DATA FILES, AND/OR
SOFTWARE, YOU UNEQUIVOCALLY ACCEPT, AND AGREE TO BE BOUND BY, ALL OF THE
TERMS AND CONDITIONS OF THIS AGREEMENT. IF YOU DO NOT AGREE, DO NOT
DOWNLOAD, INSTALL, COPY, DISTRIBUTE OR USE THE DATA FILES OR SOFTWARE.

Permission is hereby granted, free of charge, to any person obtaining a
copy of data files and any associated documentation (the "Data Files") or
software and any associated documentation (the "Software") to deal in the
Data Files or Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, and/or sell
copies of the Data Files or Software, and to permit persons to whom the
Data Files or Software are furnished to do so, provided that either (a)
this copyright and permission notice appear with all copies of the Data
Files or Software, or (b) this copyright and permission notice appear in
associated Documentation.

THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY
KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF
THIRD PARTY RIGHTS.

IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS NOTICE
BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL DAMAGES,
OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS,
WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION,
ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THE DATA
FILES OR SOFTWARE.

Except as contained in this notice, the name of a copyright holder shall
not be used in advertising or otherwise to promote the sale, use or other
dealings in these Data Files or Software without prior written
authorization of the copyright holder.

SPDX-License-Identifier: Unicode-3.0

—

Portions of ICU4X may have been adapted from ICU4C and/or ICU4J.
ICU 1.8.1 to ICU 57.1 © 1995-2016 International Business Machines Corporation and others.
```

### MPL-2.0

```text
Mozilla Public License Version 2.0
==================================

1. Definitions
--------------

1.1. "Contributor"
    means each individual or legal entity that creates, contributes to
    the creation of, or owns Covered Software.

1.2. "Contributor Version"
    means the combination of the Contributions of others (if any) used
    by a Contributor and that particular Contributor's Contribution.

1.3. "Contribution"
    means Covered Software of a particular Contributor.

1.4. "Covered Software"
    means Source Code Form to which the initial Contributor has attached
    the notice in Exhibit A, the Executable Form of such Source Code
    Form, and Modifications of such Source Code Form, in each case
    including portions thereof.

1.5. "Incompatible With Secondary Licenses"
    means

    (a) that the initial Contributor has attached the notice described
        in Exhibit B to the Covered Software; or

    (b) that the Covered Software was made available under the terms of
        version 1.1 or earlier of the License, but not also under the
        terms of a Secondary License.

1.6. "Executable Form"
    means any form of the work other than Source Code Form.

1.7. "Larger Work"
    means a work that combines Covered Software with other material, in 
    a separate file or files, that is not Covered Software.

1.8. "License"
    means this document.

1.9. "Licensable"
    means having the right to grant, to the maximum extent possible,
    whether at the time of the initial grant or subsequently, any and
    all of the rights conveyed by this License.

1.10. "Modifications"
    means any of the following:

    (a) any file in Source Code Form that results from an addition to,
        deletion from, or modification of the contents of Covered
        Software; or

    (b) any new file in Source Code Form that contains any Covered
        Software.

1.11. "Patent Claims" of a Contributor
    means any patent claim(s), including without limitation, method,
    process, and apparatus claims, in any patent Licensable by such
    Contributor that would be infringed, but for the grant of the
    License, by the making, using, selling, offering for sale, having
    made, import, or transfer of either its Contributions or its
    Contributor Version.

1.12. "Secondary License"
    means either the GNU General Public License, Version 2.0, the GNU
    Lesser General Public License, Version 2.1, the GNU Affero General
    Public License, Version 3.0, or any later versions of those
    licenses.

1.13. "Source Code Form"
    means the form of the work preferred for making modifications.

1.14. "You" (or "Your")
    means an individual or a legal entity exercising rights under this
    License. For legal entities, "You" includes any entity that
    controls, is controlled by, or is under common control with You. For
    purposes of this definition, "control" means (a) the power, direct
    or indirect, to cause the direction or management of such entity,
    whether by contract or otherwise, or (b) ownership of more than
    fifty percent (50%) of the outstanding shares or beneficial
    ownership of such entity.

2. License Grants and Conditions
--------------------------------

2.1. Grants

Each Contributor hereby grants You a world-wide, royalty-free,
non-exclusive license:

(a) under intellectual property rights (other than patent or trademark)
    Licensable by such Contributor to use, reproduce, make available,
    modify, display, perform, distribute, and otherwise exploit its
    Contributions, either on an unmodified basis, with Modifications, or
    as part of a Larger Work; and

(b) under Patent Claims of such Contributor to make, use, sell, offer
    for sale, have made, import, and otherwise transfer either its
    Contributions or its Contributor Version.

2.2. Effective Date

The licenses granted in Section 2.1 with respect to any Contribution
become effective for each Contribution on the date the Contributor first
distributes such Contribution.

2.3. Limitations on Grant Scope

The licenses granted in this Section 2 are the only rights granted under
this License. No additional rights or licenses will be implied from the
distribution or licensing of Covered Software under this License.
Notwithstanding Section 2.1(b) above, no patent license is granted by a
Contributor:

(a) for any code that a Contributor has removed from Covered Software;
    or

(b) for infringements caused by: (i) Your and any other third party's
    modifications of Covered Software, or (ii) the combination of its
    Contributions with other software (except as part of its Contributor
    Version); or

(c) under Patent Claims infringed by Covered Software in the absence of
    its Contributions.

This License does not grant any rights in the trademarks, service marks,
or logos of any Contributor (except as may be necessary to comply with
the notice requirements in Section 3.4).

2.4. Subsequent Licenses

No Contributor makes additional grants as a result of Your choice to
distribute the Covered Software under a subsequent version of this
License (see Section 10.2) or under the terms of a Secondary License (if
permitted under the terms of Section 3.3).

2.5. Representation

Each Contributor represents that the Contributor believes its
Contributions are its original creation(s) or it has sufficient rights
to grant the rights to its Contributions conveyed by this License.

2.6. Fair Use

This License is not intended to limit any rights You have under
applicable copyright doctrines of fair use, fair dealing, or other
equivalents.

2.7. Conditions

Sections 3.1, 3.2, 3.3, and 3.4 are conditions of the licenses granted
in Section 2.1.

3. Responsibilities
-------------------

3.1. Distribution of Source Form

All distribution of Covered Software in Source Code Form, including any
Modifications that You create or to which You contribute, must be under
the terms of this License. You must inform recipients that the Source
Code Form of the Covered Software is governed by the terms of this
License, and how they can obtain a copy of this License. You may not
attempt to alter or restrict the recipients' rights in the Source Code
Form.

3.2. Distribution of Executable Form

If You distribute Covered Software in Executable Form then:

(a) such Covered Software must also be made available in Source Code
    Form, as described in Section 3.1, and You must inform recipients of
    the Executable Form how they can obtain a copy of such Source Code
    Form by reasonable means in a timely manner, at a charge no more
    than the cost of distribution to the recipient; and

(b) You may distribute such Executable Form under the terms of this
    License, or sublicense it under different terms, provided that the
    license for the Executable Form does not attempt to limit or alter
    the recipients' rights in the Source Code Form under this License.

3.3. Distribution of a Larger Work

You may create and distribute a Larger Work under terms of Your choice,
provided that You also comply with the requirements of this License for
the Covered Software. If the Larger Work is a combination of Covered
Software with a work governed by one or more Secondary Licenses, and the
Covered Software is not Incompatible With Secondary Licenses, this
License permits You to additionally distribute such Covered Software
under the terms of such Secondary License(s), so that the recipient of
the Larger Work may, at their option, further distribute the Covered
Software under the terms of either this License or such Secondary
License(s).

3.4. Notices

You may not remove or alter the substance of any license notices
(including copyright notices, patent notices, disclaimers of warranty,
or limitations of liability) contained within the Source Code Form of
the Covered Software, except that You may alter any license notices to
the extent required to remedy known factual inaccuracies.

3.5. Application of Additional Terms

You may choose to offer, and to charge a fee for, warranty, support,
indemnity or liability obligations to one or more recipients of Covered
Software. However, You may do so only on Your own behalf, and not on
behalf of any Contributor. You must make it absolutely clear that any
such warranty, support, indemnity, or liability obligation is offered by
You alone, and You hereby agree to indemnify every Contributor for any
liability incurred by such Contributor as a result of warranty, support,
indemnity or liability terms You offer. You may include additional
disclaimers of warranty and limitations of liability specific to any
jurisdiction.

4. Inability to Comply Due to Statute or Regulation
---------------------------------------------------

If it is impossible for You to comply with any of the terms of this
License with respect to some or all of the Covered Software due to
statute, judicial order, or regulation then You must: (a) comply with
the terms of this License to the maximum extent possible; and (b)
describe the limitations and the code they affect. Such description must
be placed in a text file included with all distributions of the Covered
Software under this License. Except to the extent prohibited by statute
or regulation, such description must be sufficiently detailed for a
recipient of ordinary skill to be able to understand it.

5. Termination
--------------

5.1. The rights granted under this License will terminate automatically
if You fail to comply with any of its terms. However, if You become
compliant, then the rights granted under this License from a particular
Contributor are reinstated (a) provisionally, unless and until such
Contributor explicitly and finally terminates Your grants, and (b) on an
ongoing basis, if such Contributor fails to notify You of the
non-compliance by some reasonable means prior to 60 days after You have
come back into compliance. Moreover, Your grants from a particular
Contributor are reinstated on an ongoing basis if such Contributor
notifies You of the non-compliance by some reasonable means, this is the
first time You have received notice of non-compliance with this License
from such Contributor, and You become compliant prior to 30 days after
Your receipt of the notice.

5.2. If You initiate litigation against any entity by asserting a patent
infringement claim (excluding declaratory judgment actions,
counter-claims, and cross-claims) alleging that a Contributor Version
directly or indirectly infringes any patent, then the rights granted to
You by any and all Contributors for the Covered Software under Section
2.1 of this License shall terminate.

5.3. In the event of termination under Sections 5.1 or 5.2 above, all
end user license agreements (excluding distributors and resellers) which
have been validly granted by You or Your distributors under this License
prior to termination shall survive termination.

************************************************************************
*                                                                      *
*  6. Disclaimer of Warranty                                           *
*  -------------------------                                           *
*                                                                      *
*  Covered Software is provided under this License on an "as is"       *
*  basis, without warranty of any kind, either expressed, implied, or  *
*  statutory, including, without limitation, warranties that the       *
*  Covered Software is free of defects, merchantable, fit for a        *
*  particular purpose or non-infringing. The entire risk as to the     *
*  quality and performance of the Covered Software is with You.        *
*  Should any Covered Software prove defective in any respect, You     *
*  (not any Contributor) assume the cost of any necessary servicing,   *
*  repair, or correction. This disclaimer of warranty constitutes an   *
*  essential part of this License. No use of any Covered Software is   *
*  authorized under this License except under this disclaimer.         *
*                                                                      *
************************************************************************

************************************************************************
*                                                                      *
*  7. Limitation of Liability                                          *
*  --------------------------                                          *
*                                                                      *
*  Under no circumstances and under no legal theory, whether tort      *
*  (including negligence), contract, or otherwise, shall any           *
*  Contributor, or anyone who distributes Covered Software as          *
*  permitted above, be liable to You for any direct, indirect,         *
*  special, incidental, or consequential damages of any character      *
*  including, without limitation, damages for lost profits, loss of    *
*  goodwill, work stoppage, computer failure or malfunction, or any    *
*  and all other commercial damages or losses, even if such party      *
*  shall have been informed of the possibility of such damages. This   *
*  limitation of liability shall not apply to liability for death or   *
*  personal injury resulting from such party's negligence to the       *
*  extent applicable law prohibits such limitation. Some               *
*  jurisdictions do not allow the exclusion or limitation of           *
*  incidental or consequential damages, so this exclusion and          *
*  limitation may not apply to You.                                    *
*                                                                      *
************************************************************************

8. Litigation
-------------

Any litigation relating to this License may be brought only in the
courts of a jurisdiction where the defendant maintains its principal
place of business and such litigation shall be governed by laws of that
jurisdiction, without reference to its conflict-of-law provisions.
Nothing in this Section shall prevent a party's ability to bring
cross-claims or counter-claims.

9. Miscellaneous
----------------

This License represents the complete agreement concerning the subject
matter hereof. If any provision of this License is held to be
unenforceable, such provision shall be reformed only to the extent
necessary to make it enforceable. Any law or regulation which provides
that the language of a contract shall be construed against the drafter
shall not be used to construe this License against a Contributor.

10. Versions of the License
---------------------------

10.1. New Versions

Mozilla Foundation is the license steward. Except as provided in Section
10.3, no one other than the license steward has the right to modify or
publish new versions of this License. Each version will be given a
distinguishing version number.

10.2. Effect of New Versions

You may distribute the Covered Software under the terms of the version
of the License under which You originally received the Covered Software,
or under the terms of any subsequent version published by the license
steward.

10.3. Modified Versions

If you create software not governed by this License, and you want to
create a new license for such software, you may create and use a
modified version of this License if you rename the license and remove
any references to the name of the license steward (except to note that
such modified license differs from this License).

10.4. Distributing Source Code Form that is Incompatible With Secondary
Licenses

If You choose to distribute Source Code Form that is Incompatible With
Secondary Licenses under the terms of this version of the License, the
notice described in Exhibit B of this License must be attached.

Exhibit A - Source Code Form License Notice
-------------------------------------------

  This Source Code Form is subject to the terms of the Mozilla Public
  License, v. 2.0. If a copy of the MPL was not distributed with this
  file, You can obtain one at http://mozilla.org/MPL/2.0/.

If it is not possible or desirable to put the notice in a particular
file, then You may include the notice in a location (such as a LICENSE
file in a relevant directory) where a recipient would be likely to look
for such a notice.

You may add additional accurate notices of copyright ownership.

Exhibit B - "Incompatible With Secondary Licenses" Notice
---------------------------------------------------------

  This Source Code Form is "Incompatible With Secondary Licenses", as
  defined by the Mozilla Public License, v. 2.0.
```

### CDLA-Permissive-2.0

```text
# Community Data License Agreement - Permissive - Version 2.0

This is the Community Data License Agreement - Permissive, Version
2.0 (the "agreement"). Data Provider(s) and Data Recipient(s) agree
as follows:

## 1. Provision of the Data

1.1. A Data Recipient may use, modify, and share the Data made
available by Data Provider(s) under this agreement if that Data
Recipient follows the terms of this agreement.

1.2. This agreement does not impose any restriction on a Data
Recipient's use, modification, or sharing of any portions of the
Data that are in the public domain or that may be used, modified,
or shared under any other legal exception or limitation.

## 2. Conditions for Sharing Data

2.1. A Data Recipient may share Data, with or without modifications, so
long as the Data Recipient makes available the text of this agreement
with the shared Data.

## 3. No Restrictions on Results

3.1. This agreement does not impose any restriction or obligations
with respect to the use, modification, or sharing of Results.

## 4. No Warranty; Limitation of Liability

4.1. All Data Recipients receive the Data subject to the following
terms:

THE DATA IS PROVIDED ON AN "AS IS" BASIS, WITHOUT REPRESENTATIONS,
WARRANTIES OR CONDITIONS OF ANY KIND, EITHER EXPRESS OR IMPLIED
INCLUDING, WITHOUT LIMITATION, ANY WARRANTIES OR CONDITIONS OF TITLE,
NON-INFRINGEMENT, MERCHANTABILITY OR FITNESS FOR A PARTICULAR PURPOSE.

NO DATA PROVIDER SHALL HAVE ANY LIABILITY FOR ANY DIRECT, INDIRECT,
INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING
WITHOUT LIMITATION LOST PROFITS), HOWEVER CAUSED AND ON ANY THEORY OF
LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE DATA OR RESULTS,
EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGES.

## 5. Definitions

5.1. "Data" means the material received by a Data Recipient under
this agreement.

5.2. "Data Provider" means any person who is the source of Data
provided under this agreement and in reliance on a Data Recipient's
agreement to its terms.

5.3. "Data Recipient" means any person who receives Data directly
or indirectly from a Data Provider and agrees to the terms of this
agreement.

5.4. "Results" means any outcome obtained by computational analysis
of Data, including for example machine learning models and models'
insights.
```

### ring

```text
*ring* uses an "ISC" license, like BoringSSL used to use, for new code
files. See LICENSE-other-bits for the text of that license.

See LICENSE-BoringSSL for code that was sourced from BoringSSL under the
Apache 2.0 license. Some code that was sourced from BoringSSL under the ISC
license. In each case, the license info is at the top of the file.

See src/polyfill/once_cell/LICENSE-APACHE and src/polyfill/once_cell/LICENSE-MIT
for the license to code that was sourced from the once_cell project.

Copyright 2015-2025 Brian Smith.

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY
SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION
OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN
CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

Apache License
                           Version 2.0, January 2004
                        http://www.apache.org/licenses/

   TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

   1. Definitions.

      "License" shall mean the terms and conditions for use, reproduction,
      and distribution as defined by Sections 1 through 9 of this document.

      "Licensor" shall mean the copyright owner or entity authorized by
      the copyright owner that is granting the License.

      "Legal Entity" shall mean the union of the acting entity and all
      other entities that control, are controlled by, or are under common
      control with that entity. For the purposes of this definition,
      "control" means (i) the power, direct or indirect, to cause the
      direction or management of such entity, whether by contract or
      otherwise, or (ii) ownership of fifty percent (50%) or more of the
      outstanding shares, or (iii) beneficial ownership of such entity.

      "You" (or "Your") shall mean an individual or Legal Entity
      exercising permissions granted by this License.

      "Source" form shall mean the preferred form for making modifications,
      including but not limited to software source code, documentation
      source, and configuration files.

      "Object" form shall mean any form resulting from mechanical
      transformation or translation of a Source form, including but
      not limited to compiled object code, generated documentation,
      and conversions to other media types.

      "Work" shall mean the work of authorship, whether in Source or
      Object form, made available under the License, as indicated by a
      copyright notice that is included in or attached to the work
      (an example is provided in the Appendix below).

      "Derivative Works" shall mean any work, whether in Source or Object
      form, that is based on (or derived from) the Work and for which the
      editorial revisions, annotations, elaborations, or other modifications
      represent, as a whole, an original work of authorship. For the purposes
      of this License, Derivative Works shall not include works that remain
      separable from, or merely link (or bind by name) to the interfaces of,
      the Work and Derivative Works thereof.

      "Contribution" shall mean any work of authorship, including
      the original version of the Work and any modifications or additions
      to that Work or Derivative Works thereof, that is intentionally
      submitted to Licensor for inclusion in the Work by the copyright owner
      or by an individual or Legal Entity authorized to submit on behalf of
      the copyright owner. For the purposes of this definition, "submitted"
      means any form of electronic, verbal, or written communication sent
      to the Licensor or its representatives, including but not limited to
      communication on electronic mailing lists, source code control systems,
      and issue tracking systems that are managed by, or on behalf of, the
      Licensor for the purpose of discussing and improving the Work, but
      excluding communication that is conspicuously marked or otherwise
      designated in writing by the copyright owner as "Not a Contribution."

      "Contributor" shall mean Licensor and any individual or Legal Entity
      on behalf of whom a Contribution has been received by Licensor and
      subsequently incorporated within the Work.

   2. Grant of Copyright License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      copyright license to reproduce, prepare Derivative Works of,
      publicly display, publicly perform, sublicense, and distribute the
      Work and such Derivative Works in Source or Object form.

   3. Grant of Patent License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      (except as stated in this section) patent license to make, have made,
      use, offer to sell, sell, import, and otherwise transfer the Work,
      where such license applies only to those patent claims licensable
      by such Contributor that are necessarily infringed by their
      Contribution(s) alone or by combination of their Contribution(s)
      with the Work to which such Contribution(s) was submitted. If You
      institute patent litigation against any entity (including a
      cross-claim or counterclaim in a lawsuit) alleging that the Work
      or a Contribution incorporated within the Work constitutes direct
      or contributory patent infringement, then any patent licenses
      granted to You under this License for that Work shall terminate
      as of the date such litigation is filed.

   4. Redistribution. You may reproduce and distribute copies of the
      Work or Derivative Works thereof in any medium, with or without
      modifications, and in Source or Object form, provided that You
      meet the following conditions:

      (a) You must give any other recipients of the Work or
          Derivative Works a copy of this License; and

      (b) You must cause any modified files to carry prominent notices
          stating that You changed the files; and

      (c) You must retain, in the Source form of any Derivative Works
          that You distribute, all copyright, patent, trademark, and
          attribution notices from the Source form of the Work,
          excluding those notices that do not pertain to any part of
          the Derivative Works; and

      (d) If the Work includes a "NOTICE" text file as part of its
          distribution, then any Derivative Works that You distribute must
          include a readable copy of the attribution notices contained
          within such NOTICE file, excluding those notices that do not
          pertain to any part of the Derivative Works, in at least one
          of the following places: within a NOTICE text file distributed
          as part of the Derivative Works; within the Source form or
          documentation, if provided along with the Derivative Works; or,
          within a display generated by the Derivative Works, if and
          wherever such third-party notices normally appear. The contents
          of the NOTICE file are for informational purposes only and
          do not modify the License. You may add Your own attribution
          notices within Derivative Works that You distribute, alongside
          or as an addendum to the NOTICE text from the Work, provided
          that such additional attribution notices cannot be construed
          as modifying the License.

      You may add Your own copyright statement to Your modifications and
      may provide additional or different license terms and conditions
      for use, reproduction, or distribution of Your modifications, or
      for any such Derivative Works as a whole, provided Your use,
      reproduction, and distribution of the Work otherwise complies with
      the conditions stated in this License.

   5. Submission of Contributions. Unless You explicitly state otherwise,
      any Contribution intentionally submitted for inclusion in the Work
      by You to the Licensor shall be under the terms and conditions of
      this License, without any additional terms or conditions.
      Notwithstanding the above, nothing herein shall supersede or modify
      the terms of any separate license agreement you may have executed
      with Licensor regarding such Contributions.

   6. Trademarks. This License does not grant permission to use the trade
      names, trademarks, service marks, or product names of the Licensor,
      except as required for reasonable and customary use in describing the
      origin of the Work and reproducing the content of the NOTICE file.

   7. Disclaimer of Warranty. Unless required by applicable law or
      agreed to in writing, Licensor provides the Work (and each
      Contributor provides its Contributions) on an "AS IS" BASIS,
      WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
      implied, including, without limitation, any warranties or conditions
      of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A
      PARTICULAR PURPOSE. You are solely responsible for determining the
      appropriateness of using or redistributing the Work and assume any
      risks associated with Your exercise of permissions under this License.

   8. Limitation of Liability. In no event and under no legal theory,
      whether in tort (including negligence), contract, or otherwise,
      unless required by applicable law (such as deliberate and grossly
      negligent acts) or agreed to in writing, shall any Contributor be
      liable to You for damages, including any direct, indirect, special,
      incidental, or consequential damages of any character arising as a
      result of this License or out of the use or inability to use the
      Work (including but not limited to damages for loss of goodwill,
      work stoppage, computer failure or malfunction, or any and all
      other commercial damages or losses), even if such Contributor
      has been advised of the possibility of such damages.

   9. Accepting Warranty or Additional Liability. While redistributing
      the Work or Derivative Works thereof, You may choose to offer,
      and charge a fee for, acceptance of support, warranty, indemnity,
      or other liability obligations and/or rights consistent with this
      License. However, in accepting such obligations, You may act only
      on Your own behalf and on Your sole responsibility, not on behalf
      of any other Contributor, and only if You agree to indemnify,
      defend, and hold each Contributor harmless for any liability
      incurred by, or claims asserted against, such Contributor by reason
      of your accepting any such warranty or additional liability.

   END OF TERMS AND CONDITIONS

   APPENDIX: How to apply the Apache License to your work.

      To apply the Apache License to your work, attach the following
      boilerplate notice, with the fields enclosed by brackets "[]"
      replaced with your own identifying information. (Don't include
      the brackets!)  The text should be enclosed in the appropriate
      comment syntax for the file format. We also recommend that a
      file or class name and description of purpose be included on the
      same "printed page" as the copyright notice for easier
      identification within third-party archives.

   Copyright [yyyy] [name of copyright owner]

   Licensed under the Apache License, Version 2.0 (the "License");
   you may not use this file except in compliance with the License.
   You may obtain a copy of the License at

       http://www.apache.org/licenses/LICENSE-2.0

   Unless required by applicable law or agreed to in writing, software
   distributed under the License is distributed on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
   See the License for the specific language governing permissions and
   limitations under the License.


Licenses for support code
-------------------------

Parts of the TLS test suite are under the Go license. This code is not included
in BoringSSL (i.e. libcrypto and libssl) when compiled, however, so
distributing code linked against BoringSSL does not trigger this license:

Copyright (c) 2009 The Go Authors. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

   * Redistributions of source code must retain the above copyright
notice, this list of conditions and the following disclaimer.
   * Redistributions in binary form must reproduce the above
copyright notice, this list of conditions and the following disclaimer
in the documentation and/or other materials provided with the
distribution.
   * Neither the name of Google Inc. nor the names of its
contributors may be used to endorse or promote products derived from
this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.


BoringSSL uses the Chromium test infrastructure to run a continuous build,
trybots etc. The scripts which manage this, and the script for generating build
metadata, are under the Chromium license. Distributing code linked against
BoringSSL does not trigger this license.

Copyright 2015 The Chromium Authors. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

   * Redistributions of source code must retain the above copyright
notice, this list of conditions and the following disclaimer.
   * Redistributions in binary form must reproduce the above
copyright notice, this list of conditions and the following disclaimer
in the documentation and/or other materials provided with the
distribution.
   * Neither the name of Google Inc. nor the names of its
contributors may be used to endorse or promote products derived from
this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### Lucide Icons

```text
ISC License

Copyright (c) for portions of Lucide are held by Cole Bemis 2013-2022 as part of Feather (MIT). All other copyright (c) for Lucide are held by Lucide Contributors 2022.

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
```

### Tailwind CSS

```text
MIT License

Copyright (c) Tailwind Labs, Inc.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### SQLite

```text
The author disclaims copyright to this source code. In place of
a legal notice, here is a blessing:

   May you do good and not evil.
   May you find forgiveness for yourself and forgive others.
   May you share freely, never taking more than you give.
```
