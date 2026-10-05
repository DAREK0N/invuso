//! Language of a receipt (TRL-02), so translation only runs where it is
//! needed (TRL-03).
//!
//! Receipts are short, abbreviated and full of numbers, which defeats
//! general language identification. Two cheap signals work: the script
//! (kana is Japanese, Hangul Korean) and, for Latin script, the words every
//! till prints (`Summe`, `MwSt`, `TVA`, `IVA` …).

/// ISO 639-1 code of the language the texts are written in, if the
/// receipt says so clearly enough.
pub fn detect_language<'a>(texts: impl IntoIterator<Item = &'a str>) -> Option<&'static str> {
    let mut scripts = ScriptCounts::default();
    let mut scores = [0_usize; LATIN_WORDS.len()];
    for text in texts {
        for c in text.chars() {
            scripts.add(c);
        }
        for word in text
            .split(|c: char| !c.is_alphabetic() && c != '-')
            .filter(|word| !word.is_empty())
        {
            let word = word.to_lowercase();
            for (score, (_, words)) in scores.iter_mut().zip(LATIN_WORDS) {
                if words.contains(&word.as_str()) {
                    *score += 1;
                }
            }
        }
    }
    if let Some(language) = scripts.language() {
        return Some(language);
    }
    if scripts.latin < MIN_LETTERS {
        return None;
    }
    let best = *scores.iter().max()?;
    let mut leaders = scores.iter().zip(LATIN_WORDS).filter(|(s, _)| **s == best);
    match (leaders.next(), leaders.next()) {
        (Some((_, (language, _))), None) if best > 0 => Some(language),
        _ => None,
    }
}

/// Fewer letters than this say nothing about a language.
const MIN_LETTERS: usize = 3;

/// East Asian characters carry a syllable or a word each (`합계`, `小計`).
const MIN_EAST_ASIAN: usize = 2;

#[derive(Default)]
struct ScriptCounts {
    kana: usize,
    han: usize,
    hangul: usize,
    latin: usize,
}

impl ScriptCounts {
    fn add(&mut self, c: char) {
        match c {
            // Hiragana, katakana, half-width katakana.
            '\u{3041}'..='\u{309F}' | '\u{30A0}'..='\u{30FF}' | '\u{FF66}'..='\u{FF9D}' => {
                self.kana += 1;
            }
            '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' => self.han += 1,
            '\u{AC00}'..='\u{D7AF}' | '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}' => {
                self.hangul += 1;
            }
            c if c.is_alphabetic() && c.is_ascii() || ('\u{C0}'..='\u{24F}').contains(&c) => {
                self.latin += 1;
            }
            _ => {}
        }
    }

    /// The East Asian language whose script dominates. Japanese receipts
    /// mix kanji with kana; kanji alone read as Chinese.
    fn language(&self) -> Option<&'static str> {
        let east_asian = self.kana + self.han + self.hangul;
        if east_asian < MIN_EAST_ASIAN || east_asian < self.latin {
            return None;
        }
        if self.hangul > self.kana + self.han {
            Some("ko")
        } else if self.kana > 0 {
            Some("ja")
        } else {
            Some("zh")
        }
    }
}

/// Words that tell the language of a till receipt. Words shared between
/// languages (`total`, `iva`) are left out; lower case.
const LATIN_WORDS: [(&str, &[&str]); 5] = [
    (
        "de",
        &[
            "summe",
            "zwischensumme",
            "gesamt",
            "mwst",
            "ust",
            "pfand",
            "leergut",
            "stk",
            "rückgeld",
            "gegeben",
            "zahlen",
            "betrag",
            "netto",
            "brutto",
            "kartenzahlung",
            "danke",
            "einkauf",
            "rabatt",
            "steuer",
            "bar",
            "zurück",
            "endbetrag",
            "stück",
        ],
    ),
    (
        "en",
        &[
            "subtotal", "tax", "change", "cash", "thank", "you", "qty", "amount", "due", "balance",
            "tip", "receipt", "items", "paid", "tendered", "sales",
        ],
    ),
    (
        "fr",
        &[
            "tva",
            "espèces",
            "especes",
            "merci",
            "montant",
            "rendu",
            "sous-total",
            "ttc",
            "ht",
            "prix",
            "articles",
            "monnaie",
            "payer",
        ],
    ),
    (
        "es",
        &[
            "efectivo",
            "cambio",
            "gracias",
            "importe",
            "tarjeta",
            "factura",
            "base",
            "imponible",
            "entregado",
        ],
    ),
    (
        "it",
        &[
            "totale",
            "contanti",
            "resto",
            "grazie",
            "importo",
            "scontrino",
            "subtotale",
            "pagamento",
            "documento",
            "commerciale",
        ],
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_decides_east_asian_languages() {
        assert_eq!(detect_language(["生ビール 580", "小計 ¥1,160"]), Some("ja"));
        assert_eq!(detect_language(["ｵﾆｷﾞﾘ 150"]), Some("ja"));
        assert_eq!(detect_language(["합계 12,000"]), Some("ko"));
        assert_eq!(detect_language(["合计 牛肉面 25.00"]), Some("zh"));
    }

    #[test]
    fn till_words_decide_latin_languages() {
        assert_eq!(
            detect_language(["Milch 2 St 1,98", "Summe 1,98", "Gegeben 5,00"]),
            Some("de")
        );
        assert_eq!(
            detect_language(["MILK 1.99", "SUBTOTAL 1.99", "TAX 0.10", "TOTAL 2.09"]),
            Some("en")
        );
        assert_eq!(
            detect_language(["BAGUETTE 1,20", "TOTAL TTC 1,20"]),
            Some("fr")
        );
        assert_eq!(detect_language(["PAN 1,20", "EFECTIVO 5,00"]), Some("es"));
        assert_eq!(detect_language(["PANE 1,20", "TOTALE 1,20"]), Some("it"));
    }

    #[test]
    fn unclear_receipts_have_no_language() {
        assert_eq!(detect_language(["1,99", "2,50"]), None);
        assert_eq!(detect_language(["Cola 1,99", "TOTAL 1,99"]), None);
        assert_eq!(detect_language(Vec::<&str>::new()), None);
        // A Japanese word on a German receipt does not make it Japanese.
        assert_eq!(
            detect_language(["Sushi 寿司 8,90", "Summe 8,90", "MwSt 1,42"]),
            Some("de")
        );
    }
}
