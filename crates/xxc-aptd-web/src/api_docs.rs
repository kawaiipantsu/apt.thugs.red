//! Public, compiled documentation. No administrative state is read here.
use askama::Template;
use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd, html};
use std::sync::LazyLock;

pub const GUIDE: &str = include_str!("../../../docs/AUTOMATION.md");
pub const OPENAPI: &str = include_str!("../../../docs/openapi.json");

pub struct Section {
    pub id: String,
    pub title: String,
}
pub struct Guide {
    pub html: String,
    pub sections: Vec<Section>,
}
static RENDERED: LazyLock<Guide> = LazyLock::new(|| render(GUIDE));

#[derive(Template)]
#[template(path = "api.html")]
struct Page<'a> {
    site: &'a str,
    main_site: &'a str,
    version: &'static str,
    guide: &'a Guide,
}

pub fn page(state: &crate::State) -> Response {
    // Deliberately never interpolate the admin origin, proxy addresses or config.
    let page = Page {
        site: &state.config.web.site_name,
        main_site: &state.config.web.main_site_url,
        version: xxc_aptd_core::VERSION,
        guide: &RENDERED,
    };
    match page.render() {
        Ok(body) => Html(body).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Documentation unavailable",
        )
            .into_response(),
    }
}

fn render(markdown: &str) -> Guide {
    // The page owns its H1. The downloadable Markdown keeps its own title.
    let markdown = markdown
        .strip_prefix("# Project publishing API\n")
        .unwrap_or(markdown);
    let mut parser = Parser::new_ext(markdown, Options::ENABLE_TABLES).peekable();
    let mut events = Vec::new();
    let mut sections = Vec::new();
    let mut code = 0;
    while let Some(event) = parser.next() {
        match event {
            Event::Start(Tag::Heading {
                level: HeadingLevel::H2,
                ..
            }) => {
                let mut contents = Vec::new();
                let mut title = String::new();
                for item in parser.by_ref() {
                    if item == Event::End(TagEnd::Heading(HeadingLevel::H2)) {
                        break;
                    }
                    if let Event::Text(text) | Event::Code(text) = &item {
                        title.push_str(text);
                    }
                    contents.push(item);
                }
                let id = title
                    .split(|c: char| !c.is_ascii_alphanumeric())
                    .filter(|s| !s.is_empty())
                    .map(str::to_ascii_lowercase)
                    .collect::<Vec<_>>()
                    .join("-");
                events.push(Event::Start(Tag::Heading {
                    level: HeadingLevel::H2,
                    id: Some(id.clone().into()),
                    classes: vec![],
                    attrs: vec![],
                }));
                events.extend(contents.into_iter().map(safe_event));
                events.push(Event::End(TagEnd::Heading(HeadingLevel::H2)));
                sections.push(Section { id, title });
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                code += 1;
                events.push(Event::Html(format!("<div class=\"doc-code\"><button class=\"copy\" type=\"button\" data-copy=\"api-example-{code}\" aria-label=\"Copy example {code}\" hidden>copy</button><div id=\"api-example-{code}\">").into()));
                events.push(Event::Start(Tag::CodeBlock(kind)));
            }
            Event::End(TagEnd::CodeBlock) => {
                events.push(Event::End(TagEnd::CodeBlock));
                events.push(Event::Html("</div></div>".into()));
            }
            Event::Start(Tag::Table(alignment)) => {
                events.push(Event::Html("<div class=\"doc-table\" tabindex=\"0\" role=\"region\" aria-label=\"Scrollable reference table\">".into()));
                events.push(Event::Start(Tag::Table(alignment)));
            }
            Event::End(TagEnd::Table) => {
                events.push(Event::End(TagEnd::Table));
                events.push(Event::Html("</div>".into()));
            }
            other => events.push(safe_event(other)),
        }
    }
    let mut rendered = String::new();
    html::push_html(&mut rendered, events.into_iter());
    Guide {
        html: rendered.replace("<pre>", "<pre tabindex=\"0\" role=\"region\" aria-label=\"Code example; scroll with arrow keys\">"),
        sections,
    }
}

fn safe_event(event: Event<'_>) -> Event<'_> {
    match event {
        Event::Html(text) | Event::InlineHtml(text) => Event::Text(text),
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => {
            let safe = dest_url.starts_with("https://")
                || dest_url.starts_with("http://")
                || dest_url.starts_with('#')
                || (dest_url.starts_with('/') && !dest_url.starts_with("//"));
            Event::Start(Tag::Link {
                link_type,
                dest_url: if safe { dest_url } else { "#".into() },
                title,
                id,
            })
        }
        // The canonical guide has no images; avoid active content if one is added.
        Event::Start(Tag::Image {
            link_type,
            title,
            id,
            ..
        }) => Event::Start(Tag::Image {
            link_type,
            dest_url: "".into(),
            title,
            id,
        }),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compiled_guide_has_anchors_and_escaped_examples() {
        let guide = render(GUIDE);
        assert!(guide.sections.iter().any(|s| s.id == "ci-example"));
        assert!(guide.html.contains("id=\"ci-example\""));
        assert!(guide.html.contains("data-copy=\"api-example-4\""));
        assert!(!guide.html.contains("<h1>"));
        let unsafe_input = render(
            "## Unsafe <script>alert(1)</script>\n\n<script>alert(1)</script>\n\n[bad](javascript:alert)\n\n```sh\nprintf '<script>'\n```\n",
        );
        assert!(!unsafe_input.html.contains("<script>"));
        assert!(!unsafe_input.html.contains("href=\"javascript:"));
        assert!(unsafe_input.html.contains("&lt;script&gt;"));
    }
}
