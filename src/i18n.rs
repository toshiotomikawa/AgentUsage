//! Two UI languages. Interface strings are picked inline with [`Lang::t`]; strings that come from
//! the providers (window labels, errors) are written in Portuguese and translated by [`Lang::data`].

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lang {
    #[default]
    Pt,
    En,
}

/// Portuguese → English for provider-produced text.
const DATA: &[(&str, &str)] = &[
    ("5 horas", "5 hours"),
    ("semanal", "weekly"),
    ("mensal", "monthly"),
    ("diária", "daily"),
    ("curta", "short"),
    ("ciclo", "cycle"),
    ("limite", "limit"),
    ("agy falhou", "agy failed"),
    (
        "agy não conseguiu ler a cota",
        "agy could not read the quota",
    ),
    ("agy não encontrado", "agy not found"),
    ("agy não respondeu", "agy did not answer"),
    ("agy sem saída", "agy gave no output"),
    ("resposta inesperada do agy", "unexpected answer from agy"),
    (
        "sem cota de plano (login por chave de API?)",
        "no plan quota (signed in with an API key?)",
    ),
    ("resposta inesperada", "unexpected answer"),
    ("sem conexão", "offline"),
    ("sem limites informados", "no limits reported"),
    (
        "sem login por assinatura",
        "not signed in with a subscription",
    ),
    ("sem login do chatgpt", "not signed in to ChatGPT"),
    ("sem login", "not signed in"),
    ("não conectado", "not connected"),
    ("sem assinatura go", "no Go subscription"),
    ("arquivo de login ilegível", "unreadable login file"),
    (
        "login expirado — abra o agente para renovar",
        "login expired — open the agent to renew it",
    ),
    (
        "login expirado — abra o Claude Code nesta conta",
        "login expired — open Claude Code with this account",
    ),
    (
        "limite de consultas — tentando de novo em breve",
        "rate-limited — retrying soon",
    ),
];

impl Lang {
    pub fn t(self, pt: &'static str, en: &'static str) -> &'static str {
        match self {
            Lang::Pt => pt,
            Lang::En => en,
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            Lang::Pt => Lang::En,
            Lang::En => Lang::Pt,
        }
    }

    /// Short code shown on the language button.
    pub fn code(self) -> &'static str {
        self.t("PT", "EN")
    }

    /// Translate a provider string. Unknown text is returned unchanged.
    pub fn data(self, s: &str) -> String {
        if self == Lang::Pt {
            return s.to_string();
        }
        s.split(" · ")
            .map(translate_part)
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

fn translate_part(part: &str) -> String {
    if let Some(code) = part.strip_prefix("serviço respondeu ") {
        return format!("service answered {code}");
    }
    let lower = part.to_lowercase();
    let Some((_, en)) = DATA.iter().find(|(pt, _)| pt.to_lowercase() == lower) else {
        return part.to_string();
    };
    // Keep the original capitalization of the first letter.
    let upper = part.chars().next().is_some_and(char::is_uppercase);
    let mut c = en.chars();
    match c.next() {
        Some(f) if upper => f.to_uppercase().collect::<String>() + c.as_str(),
        Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_provider_text() {
        assert_eq!(Lang::En.data("Semanal"), "Weekly");
        assert_eq!(Lang::En.data("Auto · semanal"), "Auto · weekly");
        assert_eq!(Lang::En.data("sem conexão"), "offline");
        assert_eq!(
            Lang::En.data("login expirado — abra o Claude Code nesta conta"),
            "login expired — open Claude Code with this account"
        );
        assert_eq!(
            Lang::En.data("serviço respondeu 500"),
            "service answered 500"
        );
        assert_eq!(Lang::En.data("Gemini 4 Argon"), "Gemini 4 Argon");
        assert_eq!(Lang::Pt.data("Semanal"), "Semanal");
    }
}
