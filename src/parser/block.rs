#[derive(Debug)]
struct RawBlock<'a> {
    indent: &'a str,
    head: &'a str,
    subs: Vec<RawBlock<'a>>,
}

#[derive(Debug, PartialEq)]
pub struct Block<S> {
    pub head: S,
    pub subs: Vec<Block<S>>,
}

impl<'a> RawBlock<'a> {
    fn finish(self) -> Block<&'a str> {
        let RawBlock { head, subs, .. } = self;
        Block {
            head,
            subs: subs.into_iter().map(RawBlock::finish).collect(),
        }
    }
}

/// This function splits a string into the indention (spaces before text) and the real text
fn get_indent(s: &str) -> (&str, &str) {
    let mut it = s.chars();
    let mut ibc = 0;
    while let Some(x) = it.next() {
        if x.is_whitespace() {
            ibc += x.len_utf8();
        } else {
            break;
        }
    }
    s.split_at(ibc)
}

struct Parser<'a> {
    base: Vec<RawBlock<'a>>,
    stack: Vec<RawBlock<'a>>,
}

impl<'a> Parser<'a> {
    fn pop_scope_while(&mut self, mut cond: impl FnMut(&RawBlock<'a>) -> bool) {
        while !self.stack.is_empty() && cond(self.stack.last().unwrap()) {
            // merge block with parent
            self.merge_prev();
        }
    }

    fn top_indent(&self) -> &'a str {
        self.stack.last().map(|top| top.indent).unwrap_or("")
    }

    fn merge_prev(&mut self) {
        if let Some(old_top) = self.stack.pop() {
            if let Some(top2) = self.stack.last_mut() {
                top2.subs.push(old_top);
            } else {
                self.base.push(old_top);
            }
        }
    }

    fn finish(mut self) -> Vec<Block<&'a str>> {
        self.pop_scope_while(|_| true);
        assert!(self.stack.is_empty());
        self.base.into_iter().map(RawBlock::finish).collect()
    }
}

pub fn parse_nested_blocks(s: &str) -> Vec<Block<&str>> {
    let mut parser = Parser {
        base: vec![],
        stack: vec![],
    };

    for i in s
        .lines()
        .map(get_indent)
        .filter(|&(_, i)| !i.is_empty())
        .map(|(indent, head)| RawBlock {
            indent,
            head,
            subs: Vec::new(),
        })
    {
        // reduce scope if necessary
        parser.pop_scope_while(|top| !i.indent.starts_with(top.indent));
        assert!(i.indent.starts_with(parser.top_indent()));

        if i.indent == parser.top_indent() {
            // same level of indention -> same block
            parser.merge_prev();
        } else {
            // part of block $top
            // do nothing
        }
        parser.stack.push(i);
    }

    parser.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_indent() {
        assert_eq!(("  	  ", "abc"), get_indent("  	  abc"));
    }

    #[test]
    fn test_parse_nbs0() {
        assert_eq!(parse_nested_blocks(""), []);
    }

    #[test]
    fn test_parse_nbs1() {
        assert_eq!(
            parse_nested_blocks("a"),
            [Block {
                head: "a",
                subs: vec![]
            }]
        );
        assert_eq!(
            parse_nested_blocks("a\n"),
            [Block {
                head: "a",
                subs: vec![]
            }]
        );
    }

    #[test]
    fn test_parse_nbs2() {
        assert_eq!(
            parse_nested_blocks("a\nb"),
            [
                Block {
                    head: "a",
                    subs: vec![]
                },
                Block {
                    head: "b",
                    subs: vec![]
                }
            ]
        );
        assert_eq!(
            parse_nested_blocks("a\n\r\nb\n"),
            [
                Block {
                    head: "a",
                    subs: vec![]
                },
                Block {
                    head: "b",
                    subs: vec![]
                }
            ]
        );
    }

    #[test]
    fn test_parse_nbs3() {
        assert_eq!(
            parse_nested_blocks("a\n  b"),
            [Block {
                head: "a",
                subs: vec![Block {
                    head: "b",
                    subs: vec![]
                }]
            }]
        );
        assert_eq!(
            parse_nested_blocks("a\n\r\n  b\n"),
            [Block {
                head: "a",
                subs: vec![Block {
                    head: "b",
                    subs: vec![]
                }]
            }]
        );
        assert_eq!(
            parse_nested_blocks("a\n\r\n  b\nc"),
            [
                Block {
                    head: "a",
                    subs: vec![Block {
                        head: "b",
                        subs: vec![]
                    }]
                },
                Block {
                    head: "c",
                    subs: vec![]
                }
            ]
        );
    }
}
