//! Plain-text and HTML messages containing only public links and human-facing text.

/// The two languages supplied by the server's mail templates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Language {
    English,
    Korean,
}

impl Language {
    /// The language of the mail that `tag` names (`ko`, `ko-KR`, `en-GB`), or `None`
    /// for one the mail is not written in (`ja`) and for `auto`.
    pub(crate) fn written_in(tag: &str) -> Option<Self> {
        let language = tag.split('-').next().unwrap_or_default();
        if language.eq_ignore_ascii_case("ko") {
            Some(Self::Korean)
        } else if language.eq_ignore_ascii_case("en") {
            Some(Self::English)
        } else {
            None
        }
    }

    /// The first of `choices`, most wanted first, that the mail is written in, or
    /// English: the recipient's setting, then the sender's setting (for invitations),
    /// then the language of the screen the request came from.
    pub(crate) fn first_of(choices: impl IntoIterator<Item = Option<Self>>) -> Self {
        choices
            .into_iter()
            .flatten()
            .next()
            .unwrap_or(Self::English)
    }
}

pub(crate) struct Content {
    pub(crate) subject: &'static str,
    pub(crate) plain: String,
    pub(crate) html: String,
}

impl Content {
    pub(crate) fn reset(link: &str, language: Language) -> Self {
        let (subject, intro, ending) = match language {
            Language::English => (
                "Reset your fvoci password",
                "Use this link to choose a new password:",
                "This link expires in one hour and can be used once. If you did not ask for it, ignore this message.",
            ),
            Language::Korean => (
                "fvoci 비밀번호 재설정",
                "다음 링크에서 새 비밀번호를 설정하세요:",
                "이 링크는 1시간 동안 한 번만 쓸 수 있습니다. 요청하지 않았다면 이 메일을 무시하세요.",
            ),
        };
        Self::link(subject, intro, ending, link, language)
    }

    pub(crate) fn invitation(
        link: &str,
        language: Language,
        role: fvoci_model::WorkspaceRole,
    ) -> Self {
        let (subject, intro) = match language {
            Language::English => (
                "You are invited to fvoci",
                "Use this invitation link to join:",
            ),
            Language::Korean => ("fvoci 초대", "다음 초대 링크로 참여하세요:"),
        };
        let ending = match (language, role) {
            (Language::English, fvoci_model::WorkspaceRole::Guest) => {
                "This link invites one person to a board and can be used once. It keeps its original expiry."
            }
            (Language::Korean, fvoci_model::WorkspaceRole::Guest) => {
                "이 링크로 한 사람을 보드에 초대합니다. 한 번만 쓸 수 있으며 원래 만료일을 유지합니다."
            }
            (Language::English, _) => {
                "This link invites members to a workspace. It keeps its original expiry."
            }
            (Language::Korean, _) => {
                "이 링크로 워크스페이스에 구성원을 초대합니다. 원래 만료일을 유지합니다."
            }
        };
        Self::link(subject, intro, ending, link, language)
    }

    fn link(
        subject: &'static str,
        intro: &str,
        ending: &str,
        link: &str,
        language: Language,
    ) -> Self {
        let tag = match language {
            Language::English => "en",
            Language::Korean => "ko",
        };
        let safe = escape(link);
        Self {
            subject,
            plain: format!("{intro}\n\n{link}\n\n{ending}\n"),
            html: format!(
                "<!doctype html><html lang=\"{tag}\"><body><p>{}</p><p><a href=\"{safe}\">{safe}</a></p><p>{}</p></body></html>",
                escape(intro),
                escape(ending)
            ),
        }
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_language_the_mail_is_written_in_is_taken_or_else_english() {
        for (tags, want) in [
            (&["ko", "en"][..], Language::Korean),
            (&["auto", "ko-KR"][..], Language::Korean),
            (&["ja", "KO"][..], Language::Korean),
            (&["ja", "auto", "EN-gb"][..], Language::English),
            (
                &["ja", "zh-Hans", "", "ko_KR", " ko"][..],
                Language::English,
            ),
            (&["en", "ko"][..], Language::English),
            (&[][..], Language::English),
        ] {
            let choices = tags.iter().map(|tag| Language::written_in(tag));
            assert_eq!(Language::first_of(choices), want, "{tags:?}");
        }
    }

    #[test]
    fn reset_messages_have_both_languages_and_escape_html() {
        for (language, subject) in [
            (Language::English, "Reset your fvoci password"),
            (Language::Korean, "fvoci 비밀번호 재설정"),
        ] {
            let content =
                Content::reset("https://example.test/reset-password#secret\"<&", language);
            assert_eq!(content.subject, subject);
            assert!(
                content
                    .plain
                    .contains("https://example.test/reset-password#")
            );
            assert!(content.html.contains("&quot;&lt;&amp;"));
            assert!(!content.html.contains("secret\"<&"));
        }
    }
}
