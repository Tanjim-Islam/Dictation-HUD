//! Language capability maps from official provider documentation (checked 2026-10-17).
//!
//! App language codes are BCP-47 style (`en`, `en-US`, `bn`, ...), plus `multi` for automatic
//! multilingual detection. Each adapter maps app codes to its own provider codes.
//!
//! Deepgram: https://developers.deepgram.com/docs/models-languages-overview
//!   Bengali (`bn`) is available only on nova-3 (changelog 2026-01-29).
//! ElevenLabs: https://elevenlabs.io/docs/overview/capabilities/speech-to-text (ISO 639-3 codes)

pub const DEEPGRAM_NOVA3: &[&str] = &[
  "multi", "en", "en-US", "en-GB", "en-AU", "en-IN", "en-NZ", "bn", "af", "ar", "hy", "as", "be", "bs", "bg", "ca",
  "zh-CN", "zh-TW", "zh-HK", "hr", "cs", "da", "nl", "nl-BE", "et", "fi", "fr", "fr-CA", "ka", "de", "de-CH", "el",
  "gu", "he", "hi", "hu", "id", "it", "ja", "kn", "kk", "ko", "lv", "lt", "mk", "ms", "mr", "mn", "ne", "no", "ps",
  "fa", "pl", "pt", "pt-BR", "pt-PT", "pa", "ro", "ru", "sr", "sk", "sl", "es", "es-419", "sv", "tl", "ta", "te", "th",
  "tr", "uk", "ur", "vi",
];

/// nova-2 `multi` covers only English and Spanish.
pub const DEEPGRAM_NOVA2: &[&str] = &[
  "multi", "en", "en-US", "en-GB", "en-AU", "en-IN", "en-NZ", "bg", "ca", "zh-CN", "zh-TW", "zh-HK", "cs", "da", "nl",
  "nl-BE", "et", "fi", "fr", "fr-CA", "de", "de-CH", "el", "hi", "hu", "id", "it", "ja", "ko", "lv", "lt", "ms", "no",
  "pl", "pt", "pt-BR", "pt-PT", "ro", "ru", "sk", "es", "es-419", "sv", "th", "tr", "uk", "vi",
];

/// (app code, ElevenLabs ISO 639-3 code). `multi` means omit `language_code` (auto-detect).
pub const ELEVENLABS_SCRIBE_V2: &[(&str, &str)] = &[
  ("multi", ""),
  ("en", "eng"),
  ("bn", "ben"),
  ("af", "afr"),
  ("am", "amh"),
  ("ar", "ara"),
  ("hy", "hye"),
  ("as", "asm"),
  ("ast", "ast"),
  ("az", "aze"),
  ("be", "bel"),
  ("bs", "bos"),
  ("bg", "bul"),
  ("my", "mya"),
  ("yue", "yue"),
  ("ca", "cat"),
  ("ceb", "ceb"),
  ("ny", "nya"),
  ("hr", "hrv"),
  ("cs", "ces"),
  ("da", "dan"),
  ("nl", "nld"),
  ("et", "est"),
  ("fil", "fil"),
  ("fi", "fin"),
  ("fr", "fra"),
  ("ff", "ful"),
  ("gl", "glg"),
  ("lg", "lug"),
  ("ka", "kat"),
  ("de", "deu"),
  ("el", "ell"),
  ("gu", "guj"),
  ("ha", "hau"),
  ("he", "heb"),
  ("hi", "hin"),
  ("hu", "hun"),
  ("is", "isl"),
  ("ig", "ibo"),
  ("id", "ind"),
  ("ga", "gle"),
  ("it", "ita"),
  ("ja", "jpn"),
  ("jv", "jav"),
  ("kea", "kea"),
  ("kn", "kan"),
  ("kk", "kaz"),
  ("km", "khm"),
  ("ko", "kor"),
  ("ku", "kur"),
  ("ky", "kir"),
  ("lo", "lao"),
  ("lv", "lav"),
  ("ln", "lin"),
  ("lt", "lit"),
  ("luo", "luo"),
  ("lb", "ltz"),
  ("mk", "mkd"),
  ("ms", "msa"),
  ("ml", "mal"),
  ("mt", "mlt"),
  ("zh", "zho"),
  ("mi", "mri"),
  ("mr", "mar"),
  ("mn", "mon"),
  ("ne", "nep"),
  ("nso", "nso"),
  ("no", "nor"),
  ("oc", "oci"),
  ("or", "ori"),
  ("ps", "pus"),
  ("fa", "fas"),
  ("pl", "pol"),
  ("pt", "por"),
  ("pa", "pan"),
  ("ro", "ron"),
  ("ru", "rus"),
  ("sr", "srp"),
  ("sn", "sna"),
  ("sd", "snd"),
  ("si", "sin"),
  ("sk", "slk"),
  ("sl", "slv"),
  ("so", "som"),
  ("es", "spa"),
  ("sw", "swa"),
  ("sv", "swe"),
  ("ta", "tam"),
  ("tg", "tgk"),
  ("te", "tel"),
  ("th", "tha"),
  ("tr", "tur"),
  ("uk", "ukr"),
  ("umb", "umb"),
  ("ur", "urd"),
  ("uz", "uzb"),
  ("vi", "vie"),
  ("cy", "cym"),
  ("wo", "wol"),
  ("xh", "xho"),
  ("zu", "zul"),
];

/// ElevenLabs code for an app code. Region variants fall back to the base language
/// (`en-US` -> `eng`). `None` means unsupported; `Some("")` means auto-detect.
pub fn elevenlabs_code(app_code: &str) -> Option<&'static str> {
  let base = app_code.split('-').next().unwrap_or(app_code);
  let lookup = |c: &str| ELEVENLABS_SCRIBE_V2.iter().find(|(a, _)| *a == c).map(|(_, e)| *e);
  lookup(app_code).or_else(|| match base {
    "zh" => lookup("zh"),
    b => lookup(b),
  })
}

/// Picks the closest supported code in `supported` for `app_code`: exact match, then the base
/// language. `None` when the language is not supported at all.
pub fn closest<'a>(supported: &[&'a str], app_code: &str) -> Option<&'a str> {
  if let Some(c) = supported.iter().find(|c| **c == app_code) {
    return Some(c);
  }
  let base = app_code.split('-').next()?;
  supported.iter().find(|c| **c == base).copied()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn bengali_is_nova3_only() {
    assert!(DEEPGRAM_NOVA3.contains(&"bn"));
    assert!(!DEEPGRAM_NOVA2.contains(&"bn"));
  }

  #[test]
  fn elevenlabs_maps_english_and_bengali() {
    assert_eq!(elevenlabs_code("en"), Some("eng"));
    assert_eq!(elevenlabs_code("en-US"), Some("eng"));
    assert_eq!(elevenlabs_code("bn"), Some("ben"));
    assert_eq!(elevenlabs_code("multi"), Some(""));
    assert_eq!(elevenlabs_code("tlh"), None);
  }

  #[test]
  fn closest_falls_back_to_base_language() {
    assert_eq!(closest(DEEPGRAM_NOVA2, "en-CA"), Some("en"));
    assert_eq!(closest(DEEPGRAM_NOVA3, "bn"), Some("bn"));
    assert_eq!(closest(DEEPGRAM_NOVA2, "bn"), None);
  }

  #[test]
  fn no_duplicate_codes() {
    for list in [DEEPGRAM_NOVA3, DEEPGRAM_NOVA2] {
      let mut v = list.to_vec();
      v.sort();
      v.dedup();
      assert_eq!(v.len(), list.len());
    }
    let mut v: Vec<_> = ELEVENLABS_SCRIBE_V2.iter().map(|(a, _)| *a).collect();
    v.sort();
    v.dedup();
    assert_eq!(v.len(), ELEVENLABS_SCRIBE_V2.len());
  }
}
