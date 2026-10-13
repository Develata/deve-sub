//! Build a compact, acyclic alias DAG and charge its logical expansion first.

use std::{collections::HashMap, rc::Rc};

use granit_parser::{Event, Marker, Parser, Span, StrInput};

use crate::{Error, ErrorKind, Limits, Value, scalar};

pub(crate) struct Node {
    pub kind: Kind,
    pub mark: Marker,
    cost: usize,
    height: usize,
    containers: usize,
    jumps: usize,
}

pub(crate) enum Kind {
    Scalar {
        value: Result<Value, Error>,
        lexical: String,
    },
    Sequence(Vec<Rc<Node>>),
    Mapping(Vec<(Rc<Node>, Rc<Node>)>),
}

struct Occurrence {
    node: Rc<Node>,
    jumps: usize,
}

struct Reader<'a> {
    parser: Parser<'a, StrInput<'a>>,
    anchors: HashMap<usize, Rc<Node>>,
    remaining: usize,
    limits: Limits,
    events: usize,
    jumps: usize,
    json_keys: bool,
    aliases: bool,
}

pub(crate) fn parse(
    input: &str,
    limits: Limits,
    json_keys: bool,
    aliases: bool,
) -> Result<Rc<Node>, Error> {
    if input.len() > limits.input_bytes {
        return Err(Error::new(ErrorKind::Input));
    }
    check_legacy_line_breaks(input)?;
    // Bound scanner recursion even when callers pass a larger custom limit.
    let nesting = limits.depth.saturating_add(1).min(129);
    let options = granit_parser::options! {
        emit_comments: false,
        flow_nesting_limit: nesting,
        block_nesting_limit: nesting,
    };
    let mut reader = Reader {
        parser: Parser::new_from_str_with_options(input, options),
        anchors: HashMap::new(),
        remaining: limits.expanded_bytes,
        limits,
        events: 0,
        jumps: 0,
        json_keys,
        aliases,
    };
    reader.expect(|e| matches!(e, Event::StreamStart))?;
    let (start, span) = reader.next()?;
    if matches!(start, Event::StreamEnd) {
        reader.charge(64, span.start)?;
        return Ok(Rc::new(Node {
            kind: Kind::Scalar {
                value: Ok(Value::Null),
                lexical: String::new(),
            },
            mark: span.start,
            cost: 64,
            height: 0,
            containers: 0,
            jumps: 0,
        }));
    }
    if !matches!(start, Event::DocumentStart(..)) {
        return Err(Error::new(ErrorKind::Document).at(span.start));
    }
    let event = reader.next()?;
    let root = reader.node(event, 0, false)?.node;
    reader.expect(|e| matches!(e, Event::DocumentEnd))?;
    // Drain through stream end; accepting at `...` silently ignores trailing
    // malformed content in several higher-level deserializers.
    reader.expect(|e| matches!(e, Event::StreamEnd))?;
    if reader.parser.next().is_some() {
        return Err(Error::new(ErrorKind::Document));
    }
    if reader.jumps > reader.events.saturating_mul(100) {
        return Err(Error::new(ErrorKind::Alias));
    }
    Ok(root)
}

pub(crate) fn check_legacy_line_breaks(input: &str) -> Result<(), Error> {
    // libyaml treats these as line breaks, while YAML 1.2/granit treats them
    // as content. This also changes comment termination, so checking scalar
    // events alone would silently lose mapping entries after a comment. Reject
    // literal characters everywhere; escaped string contents remain supported.
    if input.contains(['\u{85}', '\u{2028}', '\u{2029}']) {
        Err(Error::new(ErrorKind::AmbiguousLineBreak))
    } else {
        Ok(())
    }
}

impl<'a> Reader<'a> {
    fn next(&mut self) -> Result<(Event<'a>, Span), Error> {
        let (event, span) = self
            .parser
            .next()
            .ok_or_else(|| Error::new(ErrorKind::Document))?
            .map_err(|e| Error::new(ErrorKind::Syntax).at(*e.marker()))?;
        if !matches!(
            event,
            Event::StreamStart | Event::StreamEnd | Event::DocumentStart(..) | Event::DocumentEnd
        ) {
            self.events = self.events.saturating_add(1);
        }
        Ok((event, span))
    }

    fn expect(&mut self, matches: impl FnOnce(&Event<'a>) -> bool) -> Result<(), Error> {
        let (event, span) = self.next()?;
        if matches(&event) {
            Ok(())
        } else {
            Err(Error::new(ErrorKind::Document).at(span.start))
        }
    }

    fn charge(&mut self, bytes: usize, mark: Marker) -> Result<(), Error> {
        self.remaining = self
            .remaining
            .checked_sub(bytes)
            .ok_or_else(|| Error::new(ErrorKind::Budget).at(mark))?;
        Ok(())
    }

    fn node(
        &mut self,
        (event, span): (Event<'a>, Span),
        depth: usize,
        flow: bool,
    ) -> Result<Occurrence, Error> {
        let mark = span.start;
        if depth > self.limits.depth.min(128) {
            return Err(Error::new(ErrorKind::Depth).at(mark));
        }
        if let Event::Alias(id) = event {
            return self.alias(id, depth, mark);
        }
        self.charge(64, mark)?;
        let mut cost = 64;
        let mut height = 0;
        let mut containers = 0;
        let mut jumps = 0;
        let (kind, anchor) = match event {
            Event::Scalar(text, style, anchor, tag) => {
                scalar::check_flow_indicator(&text, style, flow).map_err(|e| e.at(mark))?;
                // Granit represents missing scalars as a zero-width plain ~.
                // Restore empty text before tag resolution (notably !!str).
                let text = if style == granit_parser::ScalarStyle::Plain && span.start == span.end {
                    std::borrow::Cow::Borrowed("")
                } else {
                    text
                };
                let value = scalar::resolve(&text, style, tag.as_deref()).map_err(|e| e.at(mark));
                if let Err(error) = &value
                    && (!self.json_keys || error.kind() != ErrorKind::Type)
                {
                    return Err(error.clone());
                }
                // JSON's string-key visitor retains spelling even for a scalar
                // outside the number range. Charge that spelling for aliases;
                // ordered templates retain their original decoded-value budget.
                let bytes = if self.json_keys {
                    text.len()
                } else if let Ok(Value::String(s)) = &value {
                    s.len()
                } else {
                    0
                };
                self.charge(bytes, mark)?;
                cost += bytes;
                let lexical = text.into_owned();
                (Kind::Scalar { value, lexical }, anchor)
            }
            Event::SequenceStart(style, anchor, tag) => {
                scalar::container_tag(tag.as_deref(), "seq").map_err(|e| e.at(mark))?;
                self.container_depth(depth, mark)?;
                containers = 1;
                let mut children = Vec::new();
                loop {
                    let event = self.next()?;
                    if matches!(event.0, Event::SequenceEnd) {
                        break;
                    }
                    let child = self.node(
                        event,
                        depth + 1,
                        flow || style == granit_parser::StructureStyle::Flow,
                    )?;
                    accumulate(&child, &mut cost, &mut height, &mut containers, &mut jumps)?;
                    children.push(child.node);
                }
                (Kind::Sequence(children), anchor)
            }
            Event::MappingStart(style, anchor, tag) => {
                scalar::container_tag(tag.as_deref(), "map").map_err(|e| e.at(mark))?;
                self.container_depth(depth, mark)?;
                containers = 1;
                let mut pairs = Vec::new();
                loop {
                    let event = self.next()?;
                    if matches!(event.0, Event::MappingEnd) {
                        break;
                    }
                    let in_flow = flow || style == granit_parser::StructureStyle::Flow;
                    let key = self.node(event, depth + 1, in_flow)?;
                    let event = self.next()?;
                    let value = self.node(event, depth + 1, in_flow)?;
                    for child in [&key, &value] {
                        accumulate(child, &mut cost, &mut height, &mut containers, &mut jumps)?;
                    }
                    pairs.push((key.node, value.node));
                }
                (Kind::Mapping(pairs), anchor)
            }
            _ => return Err(Error::new(ErrorKind::Syntax).at(mark)),
        };
        let node = Rc::new(Node {
            kind,
            mark,
            cost,
            height,
            containers,
            jumps,
        });
        if anchor != 0 {
            self.anchors.insert(anchor, Rc::clone(&node));
        }
        Ok(Occurrence { node, jumps })
    }

    fn container_depth(&self, depth: usize, mark: Marker) -> Result<(), Error> {
        // The legacy general deserializer allowed 128 containers, including
        // empty containers; the template's stricter value-depth check is above.
        if depth >= 128 {
            Err(Error::new(ErrorKind::Depth).at(mark))
        } else {
            Ok(())
        }
    }

    fn alias(&mut self, id: usize, depth: usize, mark: Marker) -> Result<Occurrence, Error> {
        if !self.aliases {
            return Err(Error::new(ErrorKind::Alias).at(mark));
        }
        // Anchors become visible only once complete. Recursive aliases cannot
        // form a cycle, and aliases share the node directly without wrapper chains.
        let node = self
            .anchors
            .get(&id)
            .cloned()
            .ok_or_else(|| Error::new(ErrorKind::Alias).at(mark))?;
        if node.height > self.limits.depth.min(128).saturating_sub(depth)
            || node.containers > 128 - depth
        {
            return Err(Error::new(ErrorKind::Depth).at(mark));
        }
        self.charge(node.cost, mark)?;
        let jumps = node
            .jumps
            .checked_add(1)
            .ok_or_else(|| Error::new(ErrorKind::Alias).at(mark))?;
        self.jumps = self
            .jumps
            .checked_add(jumps)
            .ok_or_else(|| Error::new(ErrorKind::Alias).at(mark))?;
        Ok(Occurrence { node, jumps })
    }
}

fn accumulate(
    child: &Occurrence,
    cost: &mut usize,
    height: &mut usize,
    containers: &mut usize,
    jumps: &mut usize,
) -> Result<(), Error> {
    *cost = cost
        .checked_add(child.node.cost)
        .ok_or_else(|| Error::new(ErrorKind::Budget))?;
    *height = (*height).max(1 + child.node.height);
    *containers = (*containers).max(1 + child.node.containers);
    *jumps = jumps
        .checked_add(child.jumps)
        .ok_or_else(|| Error::new(ErrorKind::Alias))?;
    Ok(())
}
