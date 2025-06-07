use std::cell::RefCell;

use html5ever::{
    Attribute,
    tokenizer::{Tag, TagKind::StartTag, Token, TokenSink, TokenSinkResult},
};

const BASE_URL: &str = "https://www.fia.com";

enum ParserState {
    None,
    BeginEvent,
    EventTitle,
    Document,
    DocumentTitle,
    DocumentDate,
    Next,
}

#[derive(Clone, Debug)]
pub struct ParserDocument {
    pub title: Option<String>,
    pub url: Option<String>,
    pub date: Option<String>,
}

#[derive(Debug)]
pub struct Season {
    pub year: i32,
    pub events: Vec<ParserEvent>,
}

#[derive(Debug)]
pub struct ParserEvent {
    pub title: Option<String>,
    pub season: Option<i32>,
    pub documents: Vec<ParserDocument>,
}

pub struct HTMLParser<'a> {
    state: RefCell<ParserState>,
    pub season: &'a RefCell<Season>,
    event: RefCell<Option<ParserEvent>>,
    document: RefCell<Option<ParserDocument>>,
}

impl<'a> HTMLParser<'a> {
    pub fn new(season: &'a RefCell<Season>) -> Self {
        Self {
            state: RefCell::new(ParserState::None),
            season,
            event: RefCell::new(None),
            document: RefCell::new(None),
        }
    }
}

fn attr_ref<'a>(tag: &'a Tag, name: &str) -> Option<&'a Attribute> {
    tag.attrs.iter().find(|f| f.name.local.as_ref() == name)
}

impl TokenSink for HTMLParser<'_> {
    type Handle = ();

    fn process_token(&self, token: Token, _line_number: u64) -> TokenSinkResult<Self::Handle> {
        let mut parser_state = self.state.borrow_mut();
        let mut document = self.document.borrow_mut();
        let mut season = self.season.borrow_mut();
        let mut parser_event = self.event.borrow_mut();
        match token {
            Token::TagToken(tag_token) => {
                let name = tag_token.name.as_ref();
                let class = attr_ref(&tag_token, "class");
                match (tag_token.kind, name) {
                    (StartTag, "ul") => {
                        if class.unwrap().value.as_ref() == "event-wrapper" {
                            *parser_state = ParserState::BeginEvent;
                        }
                    }
                    (StartTag, "a") => {
                        match *parser_state {
                            ParserState::Next => {}
                            _ => {
                                return TokenSinkResult::Continue;
                            }
                        }
                        if let Some(href) = attr_ref(&tag_token, "href") {
                            let href = &href.value;
                            *document = Some(ParserDocument {
                                url: Some(format!(
                                    "{}{}",
                                    BASE_URL,
                                    href.trim().replace(' ', "%20")
                                )),
                                title: None,
                                date: None,
                            });
                            *parser_state = ParserState::Document;
                        }
                    }
                    (StartTag, "div") => {
                        if class.is_none() {
                            return TokenSinkResult::Continue;
                        }
                        let class = class.as_ref().unwrap().value.as_ref();
                        match *parser_state {
                            ParserState::BeginEvent => {
                                if class.starts_with("event-title") {
                                    *parser_state = ParserState::EventTitle;
                                }
                            }
                            ParserState::Document => {
                                if class == "title" {
                                    *parser_state = ParserState::DocumentTitle;
                                }
                            }
                            _ => return TokenSinkResult::Continue,
                        }
                    }
                    (StartTag, "span") => {
                        if let ParserState::Document = *parser_state {
                            if class.as_ref().unwrap().value.as_ref() == "date-display-single" {
                                *parser_state = ParserState::DocumentDate;
                            }
                        }
                    }

                    _ => {}
                }
            }
            Token::CharacterTokens(chars) => match *parser_state {
                ParserState::EventTitle => {
                    if chars.trim().is_empty() {
                        return TokenSinkResult::Continue;
                    }
                    if let Some(event) = parser_event.take() {
                        season.events.push(event);
                    }
                    let event = ParserEvent {
                        season: Some(season.year),
                        title: Some(chars.trim().to_owned()),
                        documents: Vec::with_capacity(60),
                    };
                    *parser_state = ParserState::Next;
                    *parser_event = Some(event);
                }
                ParserState::DocumentTitle => {
                    if chars.trim().is_empty() {
                        return TokenSinkResult::Continue;
                    }
                    document.as_mut().unwrap().title = Some(chars.trim().to_owned());
                    *parser_state = ParserState::Document;
                }
                ParserState::DocumentDate => {
                    if chars.trim().is_empty() {
                        return TokenSinkResult::Continue;
                    }
                    document.as_mut().unwrap().date = Some(chars.trim().to_owned());
                    *parser_state = ParserState::Next;
                    if let Some(doc) = document.take() {
                        parser_event.as_mut().unwrap().documents.push(doc);
                    }
                }
                ParserState::Document => {}
                _ => {}
            },
            Token::EOFToken => {
                if let Some(event) = parser_event.take() {
                    season.events.push(event);
                }
            }
            _ => {}
        }
        TokenSinkResult::Continue
    }
}
