use std::collections::BTreeMap;
use std::mem;

use mime::Mime;

pub struct Feed {
    pub title: String,
    pub id: String,
    pub entries: Vec<Entry>,
}

pub struct Meta {
    pub topic: Option<String>,
    pub hubs: Vec<String>,
}

pub struct Entry {
    pub title: Option<String>,
    pub id: Option<String>,
    pub link: Option<String>,
    pub summary: Option<String>,
    pub content: Option<String>,
    pub updated: Option<i64>,
}

#[allow(clippy::large_enum_variant)]
pub enum RawFeed {
    Atom(atom::Feed),
    Rss(rss::Channel),
}

// Media type of supported syndication formats.
#[derive(Copy, Clone)]
pub enum MediaType {
    Atom,
    Rss,
    Xml,
}

const NS_ATOM: &str = "http://www.w3.org/2005/Atom";

impl Feed {
    pub fn parse(kind: MediaType, content: &[u8]) -> Option<Self> {
        RawFeed::parse(kind, content).map(Into::into)
    }
}

impl From<atom::Feed> for Feed {
    fn from(feed: atom::Feed) -> Self {
        Feed {
            title: feed.title.value,
            id: feed.id,
            entries: feed.entries.into_iter().map(Entry::from).collect(),
        }
    }
}

impl From<rss::Channel> for Feed {
    fn from(channel: rss::Channel) -> Self {
        Feed {
            title: channel.title,
            id: channel.link,
            entries: channel.items.into_iter().map(Entry::from).collect(),
        }
    }
}

impl From<RawFeed> for Feed {
    fn from(raw: RawFeed) -> Self {
        match raw {
            RawFeed::Atom(feed) => Self::from(feed),
            RawFeed::Rss(channel) => Self::from(channel),
        }
    }
}

impl From<atom::Entry> for Entry {
    fn from(entry: atom::Entry) -> Self {
        let mut links = entry.links.into_iter();
        let link = links.next();
        let link = if link.as_ref().map_or(false, |l| l.rel == "alternate") {
            link
        } else {
            links.find(|l| l.rel == "alternate").or(link)
        }
        .map(|l| l.href);
        Entry {
            title: Some(entry.title.value),
            id: Some(entry.id),
            link,
            summary: entry.summary.map(|text| text.value),
            content: entry.content.and_then(|c| c.value),
            updated: Some(entry.updated.timestamp()),
        }
    }
}

impl From<rss::Item> for Entry {
    fn from(item: rss::Item) -> Self {
        let guid = item.guid;
        let link = item.link.or_else(|| {
            guid.as_ref()
                .filter(|g| g.is_permalink())
                .map(rss::Guid::value)
                .map(str::to_owned)
        });
        Entry {
            title: item.title,
            id: guid.map(|g| g.value),
            link,
            summary: item.description,
            content: item.content,
            updated: None,
        }
    }
}

impl RawFeed {
    pub fn parse(kind: MediaType, content: &[u8]) -> Option<Self> {
        match kind {
            MediaType::Atom => atom::Feed::read_from(content).ok().map(RawFeed::Atom),
            MediaType::Rss => rss::Channel::read_from(content).ok().map(RawFeed::Rss),
            MediaType::Xml => atom::Feed::read_from(content)
                .ok()
                .map(RawFeed::Atom)
                .or_else(|| rss::Channel::read_from(content).ok().map(RawFeed::Rss)),
        }
    }

    /// Parses a [`Meta`] out of the `RawFeed`.
    ///
    /// The data will be moved out from the `RawFeed` and if you call this method twice, the result
    /// of the second call is unspecified, but `Into::<Feed>::into` will still function the same way
    /// as the original would.
    pub fn take_meta(&mut self) -> Meta {
        match self {
            RawFeed::Atom(ref mut feed) => {
                let mut topic = None;
                let mut hubs = Vec::new();
                for link in &mut feed.links {
                    match &link.rel[..] {
                        "self" => {
                            if topic.is_none() {
                                topic = Some(mem::take(&mut link.href));
                            }
                        }
                        "hub" => hubs.push(mem::take(&mut link.href)),
                        _ => {}
                    }
                }
                Meta { topic, hubs }
            }
            RawFeed::Rss(ref mut channel) => {
                take_meta_rss(&mut channel.extensions, &channel.namespaces)
            }
        }
    }
}

fn take_meta_rss(
    extensions: &mut rss::extension::ExtensionMap,
    namespaces: &BTreeMap<String, String>,
) -> Meta {
    let mut topic = None;
    let mut hubs = Vec::new();
    for (prefix, map) in extensions {
        let elms = if let Some(elms) = map.get_mut("link") {
            elms
        } else {
            continue;
        };
        let prefix_is_atom = namespaces.get(&prefix[..]).map_or(false, |s| s == NS_ATOM);
        for elm in elms {
            if let Some((_, ns)) = elm
                .attrs
                .iter()
                .find(|(k, _)| k.strip_prefix("xmlns:") == Some(prefix))
            {
                // The element has `xmlns` declaration inline as in
                // `<{prefix}:{name} xmlns:{prefix}="{ns}" ..>..</{prefix}:{name}>`
                if ns != NS_ATOM {
                    continue;
                }
            } else if !prefix_is_atom {
                continue;
            }
            if let Some(rel) = elm.attrs.get("rel") {
                match &rel[..] {
                    "hub" => {
                        if let Some(href) = elm.attrs.remove("href") {
                            hubs.push(href);
                        }
                    }
                    "self" => {
                        if topic.is_none() {
                            topic = elm.attrs.remove("href");
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    Meta { topic, hubs }
}

impl std::str::FromStr for MediaType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, ()> {
        let mime: Mime = if let Ok(m) = s.parse() {
            m
        } else {
            return Err(());
        };

        if mime.type_() == mime::APPLICATION
            && mime.subtype() == "atom"
            && mime.suffix() == Some(mime::XML)
        {
            Ok(MediaType::Atom)
        } else if mime.type_() == mime::APPLICATION
            && (mime.subtype() == "rss" || mime.subtype() == "rdf")
            && mime.suffix() == Some(mime::XML)
        {
            Ok(MediaType::Rss)
        } else if (mime.type_() == mime::APPLICATION || mime.type_() == mime::TEXT)
            && mime.subtype() == mime::XML
        {
            Ok(MediaType::Xml)
        } else {
            Err(())
        }
    }
}
