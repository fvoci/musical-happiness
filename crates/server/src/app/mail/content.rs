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

    /// The language tag of the mail, as its HTML and `Content-Language` give it.
    pub(crate) fn tag(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Korean => "ko",
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

/// The text of one message, in one language.
pub(crate) struct Content {
    pub(crate) subject: &'static str,
    pub(crate) plain: String,
    pub(crate) html: String,
    pub(crate) language: Language,
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

    /// The link that confirms the address of a registration, which signs its person in
    /// as `username`: named, so that who did not sign up sees whose registration it is.
    pub(crate) fn verification(link: &str, username: &str, language: Language) -> Self {
        let (subject, intro, ending) = match language {
            Language::English => (
                "Confirm your email address for fvoci",
                format!("Use this link to confirm your email address and sign in as {username}:"),
                "This link expires in 24 hours and can be used once. If you did not sign up, ignore this message: the sign-up is removed after seven days.",
            ),
            Language::Korean => (
                "fvoci 이메일 주소 확인",
                format!("다음 링크로 이메일 주소를 확인하고 {username}(으)로 로그인하세요:"),
                "이 링크는 24시간 동안 한 번만 쓸 수 있습니다. 가입하지 않았다면 이 메일을 무시하세요. 확인하지 않은 가입은 7일 뒤 지워집니다.",
            ),
        };
        Self::link(subject, &intro, ending, link, language)
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
                "This link is yours alone: it brings you to one board and can be used once. It ends when the invitation does."
            }
            (Language::Korean, fvoci_model::WorkspaceRole::Guest) => {
                "이 링크는 받는 분만을 위한 것으로, 보드 하나에 한 번 들어올 수 있습니다. 초대가 끝나면 함께 끝납니다."
            }
            (
                Language::English,
                fvoci_model::WorkspaceRole::Admin | fvoci_model::WorkspaceRole::Member,
            ) => "You join as a member. The link keeps the expiry it was made with.",
            (
                Language::Korean,
                fvoci_model::WorkspaceRole::Admin | fvoci_model::WorkspaceRole::Member,
            ) => "구성원으로 참여하는 링크입니다. 원래 만료일을 유지합니다.",
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
        let tag = language.tag();
        let safe = escape(link);
        Self {
            subject,
            plain: format!("{intro}\n\n{link}\n\n{ending}\n"),
            html: format!(
                "<!doctype html><html lang=\"{tag}\"><body><p>{}</p><p><a href=\"{safe}\">{safe}</a></p><p>{}</p></body></html>",
                escape(intro),
                escape(ending)
            ),
            language,
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

    #[test]
    fn verification_messages_have_both_languages_and_name_no_workspace() {
        for (language, subject) in [
            (Language::English, "Confirm your email address for fvoci"),
            (Language::Korean, "fvoci 이메일 주소 확인"),
        ] {
            let content =
                Content::verification("https://example.test/verify-email#t<", "a<b", language);
            assert_eq!(content.subject, subject);
            assert!(
                content
                    .plain
                    .contains("https://example.test/verify-email#t<")
            );
            assert!(content.html.contains("#t&lt;"), "{}", content.html);
            assert!(
                content.plain.contains("a<b"),
                "the user name: {}",
                content.plain
            );
            assert!(content.html.contains("a&lt;b"), "escaped: {}", content.html);
            assert!(
                content
                    .html
                    .contains(&format!("lang=\"{}\"", language.tag()))
            );
            for text in [content.subject, &content.plain, &content.html] {
                let text = text.to_lowercase();
                for word in UNSAID {
                    assert!(!text.contains(word), "{language:?}: {word} in {text}");
                }
            }
        }
    }

    /// Words an invitation does not use: until someone has signed up and works with
    /// others, the product does not name the workspace (the organization it may).
    const UNSAID: [&str; 2] = ["workspace", "워크스페이스"];

    #[test]
    fn invitations_name_no_workspace_in_either_language() {
        use fvoci_model::WorkspaceRole;
        for language in [Language::English, Language::Korean] {
            for role in [
                WorkspaceRole::Admin,
                WorkspaceRole::Member,
                WorkspaceRole::Guest,
            ] {
                let content = Content::invitation("https://example.test/invite/t", language, role);
                for text in [content.subject, &content.plain, &content.html] {
                    let text = text.to_lowercase();
                    for word in UNSAID {
                        assert!(
                            !text.contains(word),
                            "{language:?} {role:?}: {word} in {text}"
                        );
                    }
                }
            }
        }
    }
}
