//! Formatted text: the two texts of a Patientenbehandlung and the text blocks inserted into
//! them (migration `0030`).
//!
//! The document is the editor's own (ProseMirror JSON as TipTap writes it), cut down to what the
//! vet asked for: paragraphs with bold, italic and underlined text and line breaks, and bullet
//! or numbered lists one level deep. Anything else — a heading, a link, a nested list — is
//! refused when it is read, so only this shape is ever stored. Attributes the editor adds that
//! mean nothing here are ignored rather than refused, so a TipTap minor release does not turn
//! every save into an error.
//!
//! Every type carries its `type` as a one-value enum instead of being an internally tagged
//! variant: a paragraph has to say `"type": "paragraph"` inside a list item too, and the schema
//! stays flat enough for the generated API types.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// A formatted text, as stored and as the editor reads and writes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RichText {
    #[serde(rename = "type")]
    pub kind: DocType,
    pub content: Vec<Block>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum DocType {
    #[serde(rename = "doc")]
    Doc,
}

/// What a document is made of: paragraphs and lists, in reading order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum Block {
    Paragraph(Paragraph),
    List(List),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Paragraph {
    #[serde(rename = "type")]
    pub kind: ParagraphType,
    /// Absent on an empty paragraph — a blank line the vet left.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Inline>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum ParagraphType {
    #[serde(rename = "paragraph")]
    Paragraph,
}

/// A bullet or numbered list. Its items hold paragraphs only — no list inside a list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct List {
    #[serde(rename = "type")]
    pub kind: ListType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attrs: Option<ListAttrs>,
    pub content: Vec<ListItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum ListType {
    #[serde(rename = "bulletList")]
    BulletList,
    #[serde(rename = "orderedList")]
    OrderedList,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ListAttrs {
    /// The first number of a numbered list; 1 when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ListItem {
    #[serde(rename = "type")]
    pub kind: ListItemType,
    pub content: Vec<Paragraph>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum ListItemType {
    #[serde(rename = "listItem")]
    ListItem,
}

/// What a paragraph is made of: runs of text and line breaks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum Inline {
    Text(Text),
    HardBreak(HardBreak),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Text {
    #[serde(rename = "type")]
    pub kind: TextType,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub marks: Vec<Mark>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum TextType {
    #[serde(rename = "text")]
    Text,
}

/// A line break inside a paragraph (Shift+Enter).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct HardBreak {
    #[serde(rename = "type")]
    pub kind: HardBreakType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum HardBreakType {
    #[serde(rename = "hardBreak")]
    HardBreak,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Mark {
    #[serde(rename = "type")]
    pub kind: MarkType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum MarkType {
    Bold,
    Italic,
    Underline,
}

impl RichText {
    /// The document as it is stored, or `None` when it holds no text at all — an emptied field
    /// is NULL, never a document of blank paragraphs (the CHECKs of migration `0030`).
    ///
    /// Runs of empty text, items without a paragraph and lists without items are dropped on the
    /// way: the editor never writes them, and it could not show them if it read them back.
    pub fn normalised(self) -> Option<RichText> {
        let content: Vec<Block> = self
            .content
            .into_iter()
            .filter_map(|block| match block {
                Block::Paragraph(paragraph) => Some(Block::Paragraph(paragraph.normalised())),
                Block::List(list) => {
                    let items: Vec<ListItem> = list
                        .content
                        .into_iter()
                        .filter(|item| !item.content.is_empty())
                        .map(|item| ListItem {
                            kind: item.kind,
                            content: item
                                .content
                                .into_iter()
                                .map(Paragraph::normalised)
                                .collect(),
                        })
                        .collect();
                    (!items.is_empty()).then_some(Block::List(List {
                        kind: list.kind,
                        attrs: list.attrs,
                        content: items,
                    }))
                }
            })
            .collect();
        let document = RichText {
            kind: DocType::Doc,
            content,
        };
        (!document.is_blank()).then_some(document)
    }

    /// `true` when nothing but whitespace was written.
    pub fn is_blank(&self) -> bool {
        self.paragraphs().into_iter().all(|paragraph| {
            paragraph.content.iter().all(|inline| match inline {
                Inline::Text(text) => text.text.trim().is_empty(),
                Inline::HardBreak(_) => true,
            })
        })
    }

    /// Every paragraph, list items' included, in reading order.
    fn paragraphs(&self) -> Vec<&Paragraph> {
        self.content
            .iter()
            .flat_map(|block| match block {
                Block::Paragraph(paragraph) => vec![paragraph],
                Block::List(list) => list.content.iter().flat_map(|item| &item.content).collect(),
            })
            .collect()
    }

    /// The text without its formatting, for whatever shows it as a plain string: the visit
    /// history's one-line summary, and invoice templates written before formatting existed.
    ///
    /// A paragraph or list item is a line, a line break inside one is a line break; list items
    /// keep a `• ` or their number, so a numbered instruction still reads as one. Blank lines
    /// around the text are dropped, blank lines inside it are kept.
    pub fn plain_text(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        for block in &self.content {
            match block {
                Block::Paragraph(paragraph) => lines.push(paragraph.plain_text()),
                Block::List(list) => {
                    for (marker, item) in list.markers().zip(&list.content) {
                        let text = item
                            .content
                            .iter()
                            .map(Paragraph::plain_text)
                            .collect::<Vec<_>>()
                            .join("\n");
                        lines.push(format!("{marker} {text}"));
                    }
                }
            }
        }
        trim_blank_lines(&lines).join("\n")
    }

    /// The document the way the invoice template lays it out ([`PrintBlock`]): blank paragraphs
    /// around the text dropped, formatting spelt out per run of text.
    pub fn print_blocks(&self) -> Vec<PrintBlock> {
        let blocks: Vec<PrintBlock> = self
            .content
            .iter()
            .map(|block| match block {
                Block::Paragraph(paragraph) => PrintBlock::Paragraph {
                    spans: paragraph.spans(),
                },
                Block::List(list) => PrintBlock::List {
                    numbered: list.kind == ListType::OrderedList,
                    start: list.start(),
                    items: list
                        .content
                        .iter()
                        .map(|item| {
                            // An item of several paragraphs prints as one, its paragraphs on
                            // lines of their own.
                            let mut spans = Vec::new();
                            for (index, paragraph) in item.content.iter().enumerate() {
                                if index > 0 {
                                    spans.push(Span::plain("\n"));
                                }
                                spans.extend(paragraph.spans());
                            }
                            spans
                        })
                        .collect(),
                },
            })
            .collect();
        let empty = |block: &PrintBlock| {
            matches!(block, PrintBlock::Paragraph { spans }
                if spans.iter().all(|span| span.text.trim().is_empty()))
        };
        let first = blocks.iter().position(|block| !empty(block));
        let last = blocks.iter().rposition(|block| !empty(block));
        match (first, last) {
            (Some(first), Some(last)) => blocks
                .into_iter()
                .skip(first)
                .take(last + 1 - first)
                .collect(),
            _ => Vec::new(),
        }
    }
}

impl Paragraph {
    fn normalised(self) -> Paragraph {
        Paragraph {
            kind: self.kind,
            content: self
                .content
                .into_iter()
                .filter(|inline| !matches!(inline, Inline::Text(text) if text.text.is_empty()))
                .collect(),
        }
    }

    fn plain_text(&self) -> String {
        self.content
            .iter()
            .map(|inline| match inline {
                Inline::Text(text) => text.text.as_str(),
                Inline::HardBreak(_) => "\n",
            })
            .collect()
    }

    fn spans(&self) -> Vec<Span> {
        self.content
            .iter()
            .map(|inline| match inline {
                Inline::Text(text) => Span {
                    text: text.text.clone(),
                    bold: text.has(MarkType::Bold),
                    italic: text.has(MarkType::Italic),
                    underline: text.has(MarkType::Underline),
                },
                Inline::HardBreak(_) => Span::plain("\n"),
            })
            .collect()
    }
}

impl List {
    fn start(&self) -> u32 {
        self.attrs
            .as_ref()
            .and_then(|attrs| attrs.start)
            .unwrap_or(1)
    }

    /// `•` for every item of a bullet list, `1.`, `2.`, … from the list's start for a numbered
    /// one.
    fn markers(&self) -> impl Iterator<Item = String> + '_ {
        (0..).map(move |index: u32| match self.kind {
            ListType::BulletList => "•".to_owned(),
            ListType::OrderedList => format!("{}.", self.start().saturating_add(index)),
        })
    }
}

impl Text {
    fn has(&self, mark: MarkType) -> bool {
        self.marks.iter().any(|candidate| candidate.kind == mark)
    }
}

fn trim_blank_lines(lines: &[String]) -> &[String] {
    let first = lines.iter().position(|line| !line.trim().is_empty());
    let last = lines.iter().rposition(|line| !line.trim().is_empty());
    match (first, last) {
        (Some(first), Some(last)) => &lines[first..=last],
        _ => &[],
    }
}

/// What a column of formatted text is written with: the document normalised, and `None` — NULL
/// — when it holds no text, which is what the CHECKs of migration `0030` insist on.
pub fn column(document: Option<RichText>) -> Option<sqlx::types::Json<RichText>> {
    document
        .and_then(RichText::normalised)
        .map(sqlx::types::Json)
}

/// One block of formatted text as the invoice template receives it (`docs/templates.md`).
///
/// Simpler than the stored document on purpose: a template author gets paragraphs and lists of
/// runs with their formatting spelt out, and never meets the editor's node types.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PrintBlock {
    Paragraph {
        spans: Vec<Span>,
    },
    List {
        numbered: bool,
        /// The first number of a numbered list.
        start: u32,
        /// One run of spans per item.
        items: Vec<Vec<Span>>,
    },
}

/// A run of text with its formatting. A line break is a span whose text is `"\n"`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

impl Span {
    fn plain(text: &str) -> Span {
        Span {
            text: text.to_owned(),
            bold: false,
            italic: false,
            underline: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(value: serde_json::Value) -> Result<RichText, serde_json::Error> {
        serde_json::from_value(value)
    }

    /// What TipTap writes for a bold word, a line break, an empty line and a numbered list.
    fn tiptap() -> serde_json::Value {
        json!({"type": "doc", "content": [
            {"type": "paragraph", "attrs": {"textAlign": null}, "content": [
                {"type": "text", "text": "Ohr "},
                {"type": "text", "text": "gerötet", "marks": [{"type": "bold"}, {"type": "italic"}]},
                {"type": "hardBreak"},
                {"type": "text", "text": "links", "marks": [{"type": "underline"}]}]},
            {"type": "paragraph"},
            {"type": "orderedList", "attrs": {"start": 3, "type": null}, "content": [
                {"type": "listItem", "content": [
                    {"type": "paragraph", "content": [{"type": "text", "text": "spülen"}]}]},
                {"type": "listItem", "content": [
                    {"type": "paragraph", "content": [{"type": "text", "text": "tropfen"}]}]}]}]})
    }

    #[test]
    fn reads_what_the_editor_writes_and_drops_attributes_it_does_not_know() {
        let document = parse(tiptap()).unwrap();
        let stored = serde_json::to_value(&document).unwrap();
        // `textAlign` and the list's `type` mean nothing here and are not stored.
        assert_eq!(stored["content"][0].get("attrs"), None);
        assert_eq!(stored["content"][2]["attrs"], json!({"start": 3}));
        assert_eq!(parse(stored).unwrap(), document);
    }

    #[test]
    fn refuses_what_the_editor_was_not_given() {
        let heading = json!({"type": "doc", "content": [
            {"type": "heading", "attrs": {"level": 1}, "content": [{"type": "text", "text": "x"}]}]});
        assert!(parse(heading).is_err());

        let link = json!({"type": "doc", "content": [{"type": "paragraph", "content": [
            {"type": "text", "text": "x", "marks": [{"type": "link", "attrs": {"href": "y"}}]}]}]});
        assert!(parse(link).is_err());

        // One level of list only: a list inside an item is not a paragraph.
        let nested = json!({"type": "doc", "content": [{"type": "bulletList", "content": [
            {"type": "listItem", "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "a"}]},
                {"type": "bulletList", "content": [{"type": "listItem", "content": [
                    {"type": "paragraph", "content": [{"type": "text", "text": "b"}]}]}]}]}]}]});
        assert!(parse(nested).is_err());

        assert!(parse(json!({"type": "paragraph", "content": []})).is_err());
    }

    #[test]
    fn a_document_without_text_is_no_document() {
        let blank = parse(json!({"type": "doc", "content": [
            {"type": "paragraph"},
            {"type": "paragraph", "content": [{"type": "text", "text": "  "}, {"type": "hardBreak"}]},
            {"type": "bulletList", "content": [{"type": "listItem", "content": [{"type": "paragraph"}]}]}]}))
        .unwrap();
        assert!(blank.is_blank());
        assert_eq!(blank.normalised(), None);

        assert!(parse(tiptap()).unwrap().normalised().is_some());
    }

    #[test]
    fn normalising_drops_what_the_editor_could_not_show() {
        let document = parse(json!({"type": "doc", "content": [
            {"type": "paragraph", "content": [
                {"type": "text", "text": ""}, {"type": "text", "text": "a"}]},
            {"type": "bulletList", "content": []},
            {"type": "orderedList", "content": [{"type": "listItem", "content": []}]}]}))
        .unwrap();
        let stored = serde_json::to_value(document.normalised().unwrap()).unwrap();
        assert_eq!(
            stored,
            json!({"type": "doc", "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "a"}]}]})
        );
    }

    #[test]
    fn plain_text_keeps_lines_and_list_markers() {
        let document = parse(tiptap()).unwrap();
        assert_eq!(
            document.plain_text(),
            "Ohr gerötet\nlinks\n\n3. spülen\n4. tropfen"
        );

        let bullets = parse(json!({"type": "doc", "content": [
            {"type": "paragraph"},
            {"type": "bulletList", "content": [{"type": "listItem", "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "Ruhe"}]}]}]},
            {"type": "paragraph"}]}))
        .unwrap();
        assert_eq!(bullets.plain_text(), "• Ruhe");
    }

    #[test]
    fn print_blocks_spell_out_the_formatting() {
        let blocks = parse(tiptap()).unwrap().print_blocks();
        let span = |text: &str, bold, italic, underline| Span {
            text: text.to_owned(),
            bold,
            italic,
            underline,
        };
        assert_eq!(
            blocks,
            vec![
                PrintBlock::Paragraph {
                    spans: vec![
                        span("Ohr ", false, false, false),
                        span("gerötet", true, true, false),
                        span("\n", false, false, false),
                        span("links", false, false, true),
                    ]
                },
                PrintBlock::Paragraph { spans: vec![] },
                PrintBlock::List {
                    numbered: true,
                    start: 3,
                    items: vec![
                        vec![span("spülen", false, false, false)],
                        vec![span("tropfen", false, false, false)],
                    ],
                },
            ]
        );
        assert_eq!(
            serde_json::to_value(&blocks[2]).unwrap()["kind"],
            json!("list")
        );
    }

    #[test]
    fn print_blocks_leave_out_blank_lines_around_the_text() {
        let document = parse(json!({"type": "doc", "content": [
            {"type": "paragraph"},
            {"type": "bulletList", "content": [{"type": "listItem", "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "a"}]},
                {"type": "paragraph", "content": [{"type": "text", "text": "b"}]}]}]},
            {"type": "paragraph", "content": [{"type": "text", "text": " "}]}]}))
        .unwrap();
        assert_eq!(
            document.print_blocks(),
            vec![PrintBlock::List {
                numbered: false,
                start: 1,
                items: vec![vec![Span::plain("a"), Span::plain("\n"), Span::plain("b")]],
            }]
        );
    }
}
