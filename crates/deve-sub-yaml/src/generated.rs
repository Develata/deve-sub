//! Validate already generated YAML without imposing incoming-source byte limits.

use std::collections::HashSet;

use granit_parser::{Event, Parser, Span, StrInput};

use crate::{Error, ErrorKind, Mapping, Value, scalar};

/// Shape of a named top-level field after validating the complete document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SequenceState {
    /// The root has no such field.
    Missing,
    /// The field is present but is not a sequence.
    NotSequence,
    /// The field is an empty sequence.
    Empty,
    /// The field contains at least one sequence entry.
    NonEmpty,
}

/// Check the whole single-document output and inspect a top-level sequence.
///
/// This is exclusively for emitter output: aliases are rejected and no alias
/// expansion or complete value tree is allocated. Mapping keys are retained
/// only for duplicate detection. User input must use the bounded input APIs.
pub fn inspect_generated_sequence(input: &str, key: &str) -> Result<SequenceState, Error> {
    crate::parser::check_legacy_line_breaks(input)?;
    let mut reader = Reader {
        parser: Parser::new_from_str_with_options(
            input,
            granit_parser::options! {
                emit_comments: false,
                flow_nesting_limit: 129,
                block_nesting_limit: 129,
            },
        ),
        key,
        found: SequenceState::Missing,
    };
    reader.expect(|e| matches!(e, Event::StreamStart))?;
    reader.expect(|e| matches!(e, Event::DocumentStart(..)))?;
    let event = reader.next()?;
    reader.node(event, 0, false, false)?;
    reader.expect(|e| matches!(e, Event::DocumentEnd))?;
    reader.expect(|e| matches!(e, Event::StreamEnd))?;
    if reader.parser.next().is_some() {
        return Err(Error::new(ErrorKind::Document));
    }
    Ok(reader.found)
}

struct Reader<'a, 'k> {
    parser: Parser<'a, StrInput<'a>>,
    key: &'k str,
    found: SequenceState,
}

struct Shape {
    captured: Option<Value>,
    sequence: SequenceState,
}

impl<'a> Reader<'a, '_> {
    fn next(&mut self) -> Result<(Event<'a>, Span), Error> {
        self.parser
            .next()
            .ok_or_else(|| Error::new(ErrorKind::Document))?
            .map_err(|e| Error::new(ErrorKind::Syntax).at(*e.marker()))
    }

    fn expect(&mut self, matches: impl FnOnce(&Event<'a>) -> bool) -> Result<(), Error> {
        let (event, span) = self.next()?;
        if matches(&event) {
            Ok(())
        } else {
            Err(Error::new(ErrorKind::Document).at(span.start))
        }
    }

    fn node(
        &mut self,
        (event, span): (Event<'a>, Span),
        depth: usize,
        capture: bool,
        flow: bool,
    ) -> Result<Shape, Error> {
        if depth > 128 {
            return Err(Error::new(ErrorKind::Depth).at(span.start));
        }
        match event {
            Event::Scalar(text, style, _, tag) => {
                scalar::check_flow_indicator(&text, style, flow).map_err(|e| e.at(span.start))?;
                let text = if style == granit_parser::ScalarStyle::Plain && span.start == span.end {
                    std::borrow::Cow::Borrowed("")
                } else {
                    text
                };
                let value =
                    scalar::resolve(&text, style, tag.as_deref()).map_err(|e| e.at(span.start))?;
                Ok(Shape {
                    captured: capture.then_some(value),
                    sequence: SequenceState::NotSequence,
                })
            }
            Event::SequenceStart(style, _, tag) => {
                scalar::container_tag(tag.as_deref(), "seq").map_err(|e| e.at(span.start))?;
                self.container_depth(depth, span)?;
                let mut values = Vec::new();
                let mut sequence = SequenceState::Empty;
                loop {
                    let event = self.next()?;
                    if matches!(event.0, Event::SequenceEnd) {
                        break;
                    }
                    let child = self.node(
                        event,
                        depth + 1,
                        capture,
                        flow || style == granit_parser::StructureStyle::Flow,
                    )?;
                    sequence = SequenceState::NonEmpty;
                    if let Some(value) = child.captured {
                        values.push(value);
                    }
                }
                Ok(Shape {
                    captured: capture.then_some(Value::Sequence(values)),
                    sequence,
                })
            }
            Event::MappingStart(style, _, tag) => {
                scalar::container_tag(tag.as_deref(), "map").map_err(|e| e.at(span.start))?;
                self.container_depth(depth, span)?;
                let mut values = Mapping::new();
                let mut keys = HashSet::new();
                loop {
                    let event = self.next()?;
                    if matches!(event.0, Event::MappingEnd) {
                        break;
                    }
                    let in_flow = flow || style == granit_parser::StructureStyle::Flow;
                    let key = self
                        .node(event, depth + 1, true, in_flow)?
                        .captured
                        .ok_or_else(|| Error::new(ErrorKind::Type))?;
                    if (capture && values.contains_key(&key)) || (!capture && keys.contains(&key)) {
                        return Err(Error::new(ErrorKind::DuplicateKey).at(span.start));
                    }
                    let event = self.next()?;
                    let child = self.node(event, depth + 1, capture, in_flow)?;
                    if depth == 0 && key.as_str() == Some(self.key) {
                        self.found = child.sequence;
                    }
                    if let Some(value) = child.captured {
                        values.insert(key, value);
                    } else {
                        keys.insert(key);
                    }
                }
                Ok(Shape {
                    captured: capture.then_some(Value::Mapping(values)),
                    sequence: SequenceState::NotSequence,
                })
            }
            Event::Alias(_) => Err(Error::new(ErrorKind::Alias).at(span.start)),
            _ => Err(Error::new(ErrorKind::Syntax).at(span.start)),
        }
    }

    fn container_depth(&self, depth: usize, span: Span) -> Result<(), Error> {
        if depth >= 128 {
            Err(Error::new(ErrorKind::Depth).at(span.start))
        } else {
            Ok(())
        }
    }
}
