//! Block AST and inline tree.

pub type InlineVec = Vec<Inline>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inline {
    Text(String),
    Bold(InlineVec),
    Italic(InlineVec),
    Link { text: InlineVec, url: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Paragraph(InlineVec),
    Heading {
        level: u8,
        content: InlineVec,
    },
    CodeBlock {
        lang: Option<String>,
        text: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub blocks: Vec<Block>,
    /// Bumps on every structural/text mutation; drives layout cache invalidation.
    pub generation: u64,
}

impl Inline {
    pub fn text_str(s: impl Into<String>) -> Self {
        Inline::Text(s.into())
    }

    /// Single empty text node (valid leaf for cursor).
    pub fn empty_text() -> Self {
        Inline::Text(String::new())
    }
}

impl Document {
    pub fn new() -> Self {
        Self {
            blocks: vec![Block::Paragraph(vec![Inline::empty_text()])],
            generation: 0,
        }
    }

    pub fn with_blocks(blocks: Vec<Block>) -> Self {
        Self {
            blocks,
            generation: 0,
        }
    }

    #[inline]
    pub fn bump_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}
