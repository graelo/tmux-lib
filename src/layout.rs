//! Parse the window layout string.
//!
//! Tmux reports the layout for a window, it can also use it to apply an existing layout to a
//! window.
//!
//! Up to tmux 3.7, a window layout has this format:
//!
//! ```text
//! "41e9,279x71,0,0[279x40,0,0,71,279x30,0,41{147x30,0,41,72,131x30,148,41,73}]"
//! ```
//!
//! tmux 3.8 reports a JSON subset instead, which also describes floating
//! panes. A control client still gets the format above, which leaves floating
//! panes out, unless it attaches with the `new-layouts` flag:
//!
//! ```text
//! {"V":2,"L":{"t":"h","w":120,"h":40,"x":0,"y":0,"c":[
//!   {"t":"p","w":60,"h":40,"x":0,"y":0,"a":true,"i":0,"I":"%0"},
//!   {"t":"p","w":60,"h":10,"x":-9,"y":4,"l":0,"i":1,"z":0,"I":"%1"}]}}
//! ```
//!
//! A floating pane is a leaf like any other, carrying a `z` index and
//! coordinates that overlap its siblings', and possibly negative ones.
//!
//! The parser in this module reads either and returns the corresponding
//! [`WindowLayout`].

use nom::{
    IResult, Parser,
    branch::alt,
    bytes::complete::{is_not, tag},
    character::complete::{char, digit1, hex_digit1},
    combinator::{all_consuming, map, map_opt, map_res, opt, recognize, value},
    multi::{separated_list0, separated_list1},
    sequence::{delimited, separated_pair},
};

use crate::{Result, error::map_add_intent};

/// Represent a parsed window layout.
#[derive(Debug, PartialEq, Eq)]
pub struct WindowLayout {
    /// 4-char hex checksum, such as `9f58`. The JSON format has none.
    id: Option<u16>,
    /// Container.
    container: Container,
}

impl WindowLayout {
    /// Return a flat list of pane ids.
    #[must_use]
    pub fn pane_ids(&self) -> Vec<u16> {
        let mut acc: Vec<u16> = Vec::with_capacity(1);
        self.walk(&mut acc);
        acc
    }

    /// Walk the structure, searching for pane ids.
    fn walk(&self, acc: &mut Vec<u16>) {
        self.container.walk(acc);
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Container {
    /// Dimensions of the container.
    dimensions: Dimensions,
    /// Offset of the top left corner of the container.
    coordinates: Coordinates,
    /// Either a pane, or a horizontal or vertical split.
    element: Element,
}

impl Container {
    /// Walk the structure, searching for pane ids.
    fn walk(&self, acc: &mut Vec<u16>) {
        self.element.walk(acc);
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Dimensions {
    /// Width (of the window or pane).
    width: u16,
    /// Height (of the window or pane).
    height: u16,
}

#[derive(Debug, PartialEq, Eq)]
struct Coordinates {
    /// Horizontal offset of the top left corner (of the window or pane).
    /// Negative for a floating pane moved partly off the window.
    x: i32,
    /// Vertical offset of the top left corner (of the window or pane).
    y: i32,
}

/// Element in a container.
#[derive(Debug, PartialEq, Eq)]
enum Element {
    /// A pane.
    Pane { pane_id: u16 },
    /// A horizontal split.
    Horizontal(Split),
    /// A vertical split.
    Vertical(Split),
}

impl Element {
    /// Walk the structure, searching for pane ids.
    fn walk(&self, acc: &mut Vec<u16>) {
        match self {
            Self::Pane { pane_id } => acc.push(*pane_id),
            Self::Horizontal(split) | Self::Vertical(split) => {
                split.walk(acc);
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Split {
    /// Embedded containers.
    elements: Vec<Container>,
}

impl Split {
    /// Walk the structure, searching for pane ids.
    fn walk(&self, acc: &mut Vec<u16>) {
        for element in &self.elements {
            element.walk(acc);
        }
    }
}

/// Parse the Tmux layout string description and return the pane-ids.
///
/// ```
/// use tmux_lib::layout::parse_window_layout;
///
/// let layout = parse_window_layout("9e8b,334x85,0,0{167x85,0,0,8,166x85,168,0,9}").unwrap();
/// assert_eq!(layout.pane_ids(), vec![8, 9]);
///
/// // The same layout as tmux 3.8 reports it.
/// let layout = parse_window_layout(concat!(
///     r#"{"V":2,"L":{"t":"h","w":334,"h":85,"x":0,"y":0,"c":["#,
///     r#"{"t":"p","w":167,"h":85,"x":0,"y":0,"a":true,"i":0,"I":"%8"},"#,
///     r#"{"t":"p","w":166,"h":85,"x":168,"y":0,"l":0,"i":1,"I":"%9"}]}}"#,
/// ))
/// .unwrap();
/// assert_eq!(layout.pane_ids(), vec![8, 9]);
/// ```
pub fn parse_window_layout(input: &str) -> Result<WindowLayout> {
    let desc = "window-layout";
    let intent = "window-layout";
    let (_, win_layout) = all_consuming(window_layout)
        .parse(input)
        .map_err(|e| map_add_intent(desc, intent, e))?;

    Ok(win_layout)
}

pub(crate) fn window_layout(input: &str) -> IResult<&str, WindowLayout> {
    alt((json_layout, checksummed_layout)).parse(input)
}

/// The format up to tmux 3.7: a checksum, then the root container.
fn checksummed_layout(input: &str) -> IResult<&str, WindowLayout> {
    let (input, (id, _, container)) = (layout_id, char(','), container).parse(input)?;
    Ok((
        input,
        WindowLayout {
            id: Some(id),
            container,
        },
    ))
}

fn from_hex(input: &str) -> std::result::Result<u16, std::num::ParseIntError> {
    u16::from_str_radix(input, 16)
}

fn layout_id(input: &str) -> IResult<&str, u16> {
    map_res(hex_digit1, from_hex).parse(input)
}

fn parse_u16(input: &str) -> IResult<&str, u16> {
    map_res(digit1, str::parse).parse(input)
}

fn dimensions(input: &str) -> IResult<&str, Dimensions> {
    let (input, (width, _, height)) = (parse_u16, char('x'), parse_u16).parse(input)?;
    Ok((input, Dimensions { width, height }))
}

fn parse_i32(input: &str) -> IResult<&str, i32> {
    map_res(recognize((opt(char('-')), digit1)), str::parse).parse(input)
}

fn coordinates(input: &str) -> IResult<&str, Coordinates> {
    let (input, (x, _, y)) = (parse_i32, char(','), parse_i32).parse(input)?;
    Ok((input, Coordinates { x, y }))
}

fn single_pane(input: &str) -> IResult<&str, Element> {
    let (input, (_, pane_id)) = (char(','), parse_u16).parse(input)?;
    Ok((input, Element::Pane { pane_id }))
}

fn horiz_split(input: &str) -> IResult<&str, Element> {
    let (input, elements) =
        delimited(char('{'), separated_list1(char(','), container), char('}')).parse(input)?;
    Ok((input, Element::Horizontal(Split { elements })))
}

fn vert_split(input: &str) -> IResult<&str, Element> {
    let (input, elements) =
        delimited(char('['), separated_list1(char(','), container), char(']')).parse(input)?;
    Ok((input, Element::Vertical(Split { elements })))
}

fn element(input: &str) -> IResult<&str, Element> {
    alt((single_pane, horiz_split, vert_split)).parse(input)
}

fn container(input: &str) -> IResult<&str, Container> {
    let (input, (dimensions, _, coordinates, element)) =
        (dimensions, char(','), coordinates, element).parse(input)?;
    Ok((
        input,
        Container {
            dimensions,
            coordinates,
            element,
        },
    ))
}

/// The tmux 3.8 format: `{"V":2,"L":<cell>}`.
fn json_layout(input: &str) -> IResult<&str, WindowLayout> {
    map_opt(json_value, |root| layout_from_json(&root)).parse(input)
}

/// A value in tmux's JSON subset: what its layouts hold, and nothing more.
///
/// Keys are looked up rather than expected in order, and unknown ones are
/// skipped: tmux already writes `a`, `l` and `z` only for some panes, and a
/// later release adding a key should not stop windows from being listed.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Json<'a> {
    Object(Vec<(&'a str, Json<'a>)>),
    Array(Vec<Json<'a>>),
    String(&'a str),
    Number(i64),
    Bool(bool),
}

impl<'a> Json<'a> {
    fn get(&self, key: &str) -> Option<&Json<'a>> {
        let Json::Object(members) = self else {
            return None;
        };
        members
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| value)
    }

    fn number<T: TryFrom<i64>>(&self, key: &str) -> Option<T> {
        match self.get(key)? {
            Json::Number(n) => T::try_from(*n).ok(),
            _ => None,
        }
    }

    fn string(&self, key: &str) -> Option<&'a str> {
        match self.get(key)? {
            Json::String(s) => Some(s),
            _ => None,
        }
    }
}

fn json_value(input: &str) -> IResult<&str, Json<'_>> {
    alt((
        map(json_object, Json::Object),
        map(
            delimited(char('['), separated_list0(char(','), json_value), char(']')),
            Json::Array,
        ),
        map(json_string, Json::String),
        map(
            map_res(recognize((opt(char('-')), digit1)), str::parse),
            Json::Number,
        ),
        value(Json::Bool(true), tag("true")),
        value(Json::Bool(false), tag("false")),
    ))
    .parse(input)
}

fn json_object(input: &str) -> IResult<&str, Vec<(&str, Json<'_>)>> {
    delimited(
        char('{'),
        separated_list0(
            char(','),
            separated_pair(json_string, char(':'), json_value),
        ),
        char('}'),
    )
    .parse(input)
}

/// A string without escapes.
///
/// tmux writes only cell types and pane ids as strings, so one holding a
/// backslash is rejected rather than half-read.
fn json_string(input: &str) -> IResult<&str, &str> {
    delimited(
        char('"'),
        map(opt(is_not("\"\\")), Option::unwrap_or_default),
        char('"'),
    )
    .parse(input)
}

fn layout_from_json(root: &Json<'_>) -> Option<WindowLayout> {
    if root.number::<i64>("V")? != 2 {
        return None;
    }

    Some(WindowLayout {
        id: None,
        container: container_from_json(root.get("L")?)?,
    })
}

/// One cell: `t` is `h` for a left-right split, `v` for a top-bottom one,
/// and `p` for a pane, whose id is `I`.
fn container_from_json(cell: &Json<'_>) -> Option<Container> {
    let element = match cell.string("t")? {
        "p" => Element::Pane {
            pane_id: cell.string("I")?.strip_prefix('%')?.parse().ok()?,
        },
        "h" => Element::Horizontal(split_from_json(cell)?),
        "v" => Element::Vertical(split_from_json(cell)?),
        _ => return None,
    };

    Some(Container {
        dimensions: Dimensions {
            width: cell.number("w")?,
            height: cell.number("h")?,
        },
        coordinates: Coordinates {
            x: cell.number("x")?,
            y: cell.number("y")?,
        },
        element,
    })
}

fn split_from_json(cell: &Json<'_>) -> Option<Split> {
    let Json::Array(children) = cell.get("c")? else {
        return None;
    };
    if children.is_empty() {
        return None;
    }

    Some(Split {
        elements: children
            .iter()
            .map(container_from_json)
            .collect::<Option<_>>()?,
    })
}

#[cfg(test)]
mod tests {

    use super::{
        Container, Coordinates, Dimensions, Element, Split, WindowLayout, coordinates, dimensions,
        layout_id, parse_window_layout, single_pane, vert_split, window_layout,
    };

    #[test]
    fn test_parse_layout_id() {
        let input = "9f58";

        let actual = layout_id(input);
        let expected = Ok(("", 40792_u16));
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_parse_dimensions() {
        let input = "237x0";

        let actual = dimensions(input);
        let expected = Ok((
            "",
            Dimensions {
                width: 237,
                height: 0,
            },
        ));
        assert_eq!(actual, expected);

        let input = "7x13";

        let actual = dimensions(input);
        let expected = Ok((
            "",
            Dimensions {
                width: 7,
                height: 13,
            },
        ));
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_parse_coordinates() {
        let input = "120,0";

        let actual = coordinates(input);
        let expected = Ok(("", Coordinates { x: 120, y: 0 }));
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_single_pane() {
        let input = ",46";

        let actual = single_pane(input);
        let expected = Ok(("", Element::Pane { pane_id: 46 }));
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_vertical_split() {
        let input = "[279x47,0,0,82,279x23,0,48,83]";

        let actual = vert_split(input);
        let expected = Ok((
            "",
            Element::Vertical(Split {
                elements: vec![
                    Container {
                        dimensions: Dimensions {
                            width: 279,
                            height: 47,
                        },
                        coordinates: Coordinates { x: 0, y: 0 },
                        element: Element::Pane { pane_id: 82 },
                    },
                    Container {
                        dimensions: Dimensions {
                            width: 279,
                            height: 23,
                        },
                        coordinates: Coordinates { x: 0, y: 48 },
                        element: Element::Pane { pane_id: 83 },
                    },
                ],
            }),
        ));
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_layout() {
        let input = "41e9,279x71,0,0[279x40,0,0,71,279x30,0,41{147x30,0,41,72,131x30,148,41,73}]";

        let actual = window_layout(input);
        let expected = Ok((
            "",
            WindowLayout {
                id: Some(0x41e9),
                container: Container {
                    dimensions: Dimensions {
                        width: 279,
                        height: 71,
                    },
                    coordinates: Coordinates { x: 0, y: 0 },
                    element: Element::Vertical(Split {
                        elements: vec![
                            Container {
                                dimensions: Dimensions {
                                    width: 279,
                                    height: 40,
                                },
                                coordinates: Coordinates { x: 0, y: 0 },
                                element: Element::Pane { pane_id: 71 },
                            },
                            Container {
                                dimensions: Dimensions {
                                    width: 279,
                                    height: 30,
                                },
                                coordinates: Coordinates { x: 0, y: 41 },
                                element: Element::Horizontal(Split {
                                    elements: vec![
                                        Container {
                                            dimensions: Dimensions {
                                                width: 147,
                                                height: 30,
                                            },
                                            coordinates: Coordinates { x: 0, y: 41 },
                                            element: Element::Pane { pane_id: 72 },
                                        },
                                        Container {
                                            dimensions: Dimensions {
                                                width: 131,
                                                height: 30,
                                            },
                                            coordinates: Coordinates { x: 148, y: 41 },
                                            element: Element::Pane { pane_id: 73 },
                                        },
                                    ],
                                }),
                            },
                        ],
                    }),
                },
            },
        ));
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_pane_ids() {
        let input = "41e9,279x71,0,0[279x40,0,0,71,279x30,0,41{147x30,0,41,72,131x30,148,41,73}]";
        let (_, layout) = window_layout(input).unwrap();

        let actual = layout.pane_ids();
        let expected = vec![71, 72, 73];
        assert_eq!(actual, expected);
    }

    // The JSON samples below were reported by tmux 3.8.

    /// Three tiled panes: one on the left, two stacked on the right.
    const JSON_TILED: &str = concat!(
        r#"{"V":2,"L":{"t":"h","w":120,"h":40,"x":0,"y":0,"c":["#,
        r#"{"t":"p","w":60,"h":40,"x":0,"y":0,"l":1,"i":0,"I":"%0"},"#,
        r#"{"t":"v","w":59,"h":40,"x":61,"y":0,"c":["#,
        r#"{"t":"p","w":59,"h":20,"x":61,"y":0,"l":0,"i":1,"I":"%1"},"#,
        r#"{"t":"p","w":59,"h":19,"x":61,"y":21,"a":true,"i":2,"I":"%2"}]}]}}"#,
    );

    #[test]
    fn a_json_layout_describes_the_same_tree_as_the_checksummed_one() {
        // What a control client without `new-layouts` got for the same
        // window.
        let checksummed = "95e4,120x40,0,0{60x40,0,0,0,59x40,61,0[59x20,61,0,1,59x19,61,21,2]}";

        let (_, json) = window_layout(JSON_TILED).unwrap();
        let (_, old) = window_layout(checksummed).unwrap();

        assert_eq!(json.container, old.container);
        assert_eq!(json.id, None);
    }

    #[test]
    fn a_single_pane_json_layout() {
        let input = r#"{"V":2,"L":{"t":"p","w":120,"h":40,"x":0,"y":0,"a":true,"i":0,"I":"%0"}}"#;

        assert_eq!(parse_window_layout(input).unwrap().pane_ids(), vec![0]);
    }

    #[test]
    fn floating_panes_are_listed_with_the_tiled_ones() {
        // Two floating panes sit in the right-hand container alongside the
        // tiled ones. The checksummed format leaves them out entirely.
        let input = concat!(
            r#"{"V":2,"L":{"t":"h","w":120,"h":40,"x":0,"y":0,"c":["#,
            r#"{"t":"p","w":60,"h":40,"x":0,"y":0,"l":3,"i":0,"I":"%0"},"#,
            r#"{"t":"v","w":59,"h":40,"x":61,"y":0,"c":["#,
            r#"{"t":"p","w":59,"h":20,"x":61,"y":0,"l":2,"i":1,"I":"%1"},"#,
            r#"{"t":"p","w":59,"h":19,"x":61,"y":21,"l":1,"i":2,"I":"%2"},"#,
            r#"{"t":"p","w":60,"h":10,"x":4,"y":2,"l":0,"i":3,"z":1,"I":"%3"},"#,
            r#"{"t":"p","w":60,"h":10,"x":8,"y":4,"a":true,"i":4,"z":0,"I":"%4"}]}]}}"#,
        );

        assert_eq!(
            parse_window_layout(input).unwrap().pane_ids(),
            vec![0, 1, 2, 3, 4]
        );
    }

    #[test]
    fn a_floating_pane_may_lie_partly_off_the_window() {
        let input = concat!(
            r#"{"V":2,"L":{"t":"v","w":120,"h":40,"x":0,"y":0,"c":["#,
            r#"{"t":"p","w":120,"h":40,"x":0,"y":0,"l":0,"i":0,"I":"%0"},"#,
            r#"{"t":"p","w":60,"h":10,"x":-9,"y":-4,"a":true,"i":1,"z":0,"I":"%1"}]}}"#,
        );

        assert_eq!(parse_window_layout(input).unwrap().pane_ids(), vec![0, 1]);
    }

    #[test]
    fn json_keys_may_come_in_any_order_and_unknown_ones_are_skipped() {
        let input = concat!(
            r#"{"L":{"I":"%7","new":[1,{"k":"v"}],"y":0,"x":0,"h":40,"w":80,"t":"p","f":false},"#,
            r#""V":2}"#,
        );

        assert_eq!(parse_window_layout(input).unwrap().pane_ids(), vec![7]);
    }

    #[test]
    fn a_malformed_json_layout_is_rejected() {
        let rejected = [
            // Another version of the format.
            r#"{"V":3,"L":{"t":"p","w":1,"h":1,"x":0,"y":0,"I":"%0"}}"#,
            // A pane without an id.
            r#"{"V":2,"L":{"t":"p","w":1,"h":1,"x":0,"y":0}}"#,
            // A pane id without its `%`.
            r#"{"V":2,"L":{"t":"p","w":1,"h":1,"x":0,"y":0,"I":"0"}}"#,
            // A split without children.
            r#"{"V":2,"L":{"t":"h","w":1,"h":1,"x":0,"y":0,"c":[]}}"#,
            // An unknown cell type.
            r#"{"V":2,"L":{"t":"q","w":1,"h":1,"x":0,"y":0,"I":"%0"}}"#,
            // A negative size.
            r#"{"V":2,"L":{"t":"p","w":-1,"h":1,"x":0,"y":0,"I":"%0"}}"#,
            // An escape, which tmux never writes.
            r#"{"V":2,"L":{"t":"p","w":1,"h":1,"x":0,"y":0,"I":"%\"0"}}"#,
            // Trailing bytes.
            r#"{"V":2,"L":{"t":"p","w":1,"h":1,"x":0,"y":0,"I":"%0"}}x"#,
        ];

        for input in rejected {
            assert!(parse_window_layout(input).is_err(), "accepted {input}");
        }
    }
}
