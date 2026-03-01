//! Character width calculation and line/page layout for text box simulation.

use crate::encoding::codec::{ControlCode, GameChar, Token};

/// Text box width (in 8x8 tile units).
pub const BOX_WIDTH_TILES: usize = 20;
/// Default text box lines (dialogue/battle). Diary uses 5 — see BankConfig.box_lines.
#[cfg(test)]
pub const BOX_LINES: usize = 3;

/// Get the width of a game character in 8×8 tile units.
/// The dialogue engine ($00:$CE9E) renders ALL characters as 16×16 (char × 64 bytes),
/// so every character occupies 2 tile columns regardless of encoding type.
pub fn char_width(_gc: &GameChar) -> usize {
    2
}

/// A rendered line within a text box page.
#[derive(Debug, Clone)]
pub struct Line {
    pub tokens: Vec<Token>,
    pub width: usize,
}

/// A page of text (one screen of dialogue).
#[derive(Debug, Clone)]
pub struct Page {
    pub lines: Vec<Line>,
}

/// Result of rendering a text string into pages.
#[derive(Debug)]
pub struct RenderResult {
    pub pages: Vec<Page>,
    pub overflow: bool,
    pub overflow_chars: usize,
}

/// Render a token stream into pages with default BOX_LINES (3) limit.
#[cfg(test)]
pub fn render_pages(tokens: &[Token]) -> RenderResult {
    render_pages_with_limit(tokens, BOX_LINES)
}

/// Render a token stream into pages with line wrapping.
/// `max_lines` overrides the default BOX_LINES (3) for per-bank limits (e.g. diary = 5).
pub fn render_pages_with_limit(tokens: &[Token], max_lines: usize) -> RenderResult {
    let mut pages: Vec<Page> = Vec::new();
    let mut current_lines: Vec<Line> = Vec::new();
    let mut current_line = Line {
        tokens: Vec::new(),
        width: 0,
    };
    let mut overflow = false;
    let mut overflow_chars = 0;

    for token in tokens {
        match token {
            Token::Control(ControlCode::End) => break,
            Token::Control(ControlCode::Newline) => {
                current_lines.push(current_line);
                current_line = Line {
                    tokens: Vec::new(),
                    width: 0,
                };
                // Check for line overflow within a page
                if current_lines.len() > max_lines {
                    overflow = true;
                }
            }
            Token::Control(ControlCode::PageBreak) | Token::Control(ControlCode::Separator) => {
                current_lines.push(current_line);
                pages.push(Page {
                    lines: current_lines,
                });
                current_lines = Vec::new();
                current_line = Line {
                    tokens: Vec::new(),
                    width: 0,
                };
            }
            Token::Control(ControlCode::TextBox(_)) => {
                // New text box — start a fresh page
                if !current_line.tokens.is_empty() || !current_lines.is_empty() {
                    current_lines.push(current_line);
                    pages.push(Page {
                        lines: current_lines,
                    });
                    current_lines = Vec::new();
                }
                current_line = Line {
                    tokens: Vec::new(),
                    width: 0,
                };
            }
            Token::Control(ControlCode::Space) => {
                let w = 2; // 16×16 rendered space = 2 tile columns
                if current_line.width + w > BOX_WIDTH_TILES {
                    // Wrap
                    current_lines.push(current_line);
                    current_line = Line {
                        tokens: Vec::new(),
                        width: 0,
                    };
                }
                current_line.tokens.push(token.clone());
                current_line.width += w;
            }
            Token::Char(gc, _) => {
                let w = char_width(gc);
                if current_line.width + w > BOX_WIDTH_TILES {
                    // Wrap to next line
                    current_lines.push(current_line);
                    current_line = Line {
                        tokens: Vec::new(),
                        width: 0,
                    };
                }
                current_line.tokens.push(token.clone());
                current_line.width += w;
            }
            Token::Unknown(_) | Token::UnknownFb(_) => {
                let w = 2; // 16×16 rendered = 2 tile columns
                if current_line.width + w > BOX_WIDTH_TILES {
                    current_lines.push(current_line);
                    current_line = Line {
                        tokens: Vec::new(),
                        width: 0,
                    };
                }
                current_line.tokens.push(token.clone());
                current_line.width += w;
            }
            _ => {
                current_line.tokens.push(token.clone());
            }
        }
    }

    // Flush remaining
    if !current_line.tokens.is_empty() || !current_lines.is_empty() {
        current_lines.push(current_line);
        pages.push(Page {
            lines: current_lines,
        });
    }

    // Check for overflow on each page
    for page in &pages {
        if page.lines.len() > max_lines {
            overflow = true;
            overflow_chars += page
                .lines
                .iter()
                .skip(max_lines)
                .map(|l| l.tokens.len())
                .sum::<usize>();
        }
        for line in &page.lines {
            if line.width > BOX_WIDTH_TILES {
                overflow = true;
                overflow_chars += line.width - BOX_WIDTH_TILES;
            }
        }
    }

    RenderResult {
        pages,
        overflow,
        overflow_chars,
    }
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
