use std::collections::HashMap;

pub fn normalize_language_tag(tag: &str) -> String {
    let normalized = tag.trim().to_lowercase().replace('_', "-");
    if normalized.is_empty() {
        return String::new();
    }

    let base = normalized.split('-').next().unwrap_or(&normalized);
    match base {
        "zh" | "cmn" => "zh".into(),
        "es" | "spa" => "es".into(),
        "de" | "ger" | "deu" => "de".into(),
        "en" | "eng" => "en".into(),
        other => other.into(),
    }
}

pub fn configured_language(language: &str) -> Option<String> {
    let normalized = normalize_language_tag(language);
    if normalized.is_empty() || normalized == "auto" {
        None
    } else {
        Some(normalized)
    }
}

pub fn detect_language_from_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    if trimmed.chars().any(is_cjk_char) {
        return Some("zh".into());
    }

    let lower = trimmed.to_lowercase();

    let spanish_hits = [
        " el ",
        " la ",
        " los ",
        " las ",
        " un ",
        " una ",
        " por ",
        " para ",
        " gracias ",
        "hola",
        "qué",
        "como ",
        "está",
        "estoy",
        "buenos",
        "buenas",
    ]
    .iter()
    .filter(|pattern| lower.contains(**pattern))
    .count()
        + lower.matches('ñ').count()
        + lower.matches('á').count()
        + lower.matches('é').count()
        + lower.matches('í').count()
        + lower.matches('ó').count()
        + lower.matches('ú').count();

    if spanish_hits >= 2 {
        return Some("es".into());
    }

    let german_hits = [
        " der ", " die ", " das ", " und ", " nicht ", " ich ", " ist ", " wie ", " danke ",
        "hallo", "guten", "bitte",
    ]
    .iter()
    .filter(|pattern| lower.contains(**pattern))
    .count()
        + lower.matches('ä').count()
        + lower.matches('ö').count()
        + lower.matches('ü').count()
        + lower.matches('ß').count();

    if german_hits >= 2 {
        return Some("de".into());
    }

    if lower.is_ascii() {
        Some("en".into())
    } else {
        None
    }
}

pub fn select_tts_model<'a>(
    language: Option<&str>,
    configured_models: &'a HashMap<String, String>,
    default_model: &'a str,
) -> &'a str {
    let Some(language) = language else {
        return default_model;
    };

    let normalized = normalize_language_tag(language);
    if normalized.is_empty() {
        return default_model;
    }

    configured_models
        .get(&normalized)
        .map(String::as_str)
        .or_else(|| {
            language
                .split(['-', '_'])
                .next()
                .and_then(|short| configured_models.get(short))
                .map(String::as_str)
        })
        .unwrap_or(default_model)
}

fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B820..=0x2CEAF
            | 0x2CEB0..=0x2EBEF
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_common_language_tags() {
        assert_eq!(normalize_language_tag("en-US"), "en");
        assert_eq!(normalize_language_tag("de_DE"), "de");
        assert_eq!(normalize_language_tag("zh-CN"), "zh");
    }

    #[test]
    fn configured_language_treats_auto_as_none() {
        assert_eq!(configured_language("auto"), None);
        assert_eq!(configured_language("es-ES"), Some("es".into()));
    }

    #[test]
    fn detect_language_handles_chinese() {
        assert_eq!(
            detect_language_from_text("打开客厅的灯。"),
            Some("zh".into())
        );
    }

    #[test]
    fn detect_language_handles_spanish() {
        assert_eq!(
            detect_language_from_text("hola, ¿cómo está la casa hoy?"),
            Some("es".into())
        );
    }

    #[test]
    fn detect_language_handles_german() {
        assert_eq!(
            detect_language_from_text("hallo, wie ist das wetter heute?"),
            Some("de".into())
        );
    }

    #[test]
    fn select_tts_model_prefers_language_specific_voice() {
        let mut models = HashMap::new();
        models.insert("es".into(), "/voices/es.onnx".into());
        assert_eq!(
            select_tts_model(Some("es-ES"), &models, "/voices/en.onnx"),
            "/voices/es.onnx"
        );
        assert_eq!(
            select_tts_model(Some("de"), &models, "/voices/en.onnx"),
            "/voices/en.onnx"
        );
    }
}
