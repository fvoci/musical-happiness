//! Reads Rust source and lists what it names: the paths it writes, resolved to where
//! they lead, the names it gives to paths, and the handles it reaches through.
//!
//! The reading is of the text alone, without types. What it sees and what it cannot see
//! is said at [`read_source`].

use std::collections::BTreeSet;

use anyhow::{Result, anyhow};
use proc_macro2::{Delimiter, Spacing, TokenStream, TokenTree};
use syn::ext::IdentExt as _;
use syn::visit::{self, Visit};

/// Something a module names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reference {
    /// The line it is written on, from 1.
    pub(crate) line: usize,
    /// The module the code is in, inline modules included: `fvoci_server::api::tests`.
    pub(crate) module: String,
    /// What is named.
    pub(crate) target: Target,
}

/// What a [`Reference`] names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    /// A path, made absolute: `fvoci_store::Store`, `fvoci_server::ws::Hub`. Where its
    /// first name is one the file gave with `use` or `type`, it starts with that name
    /// (`fvoci_server::api::Database::connect`). A path whose first name is neither in
    /// sight nor `crate`, `self` or `super` is kept as written: it starts at another
    /// crate.
    Path(String),
    /// A handle taken from a value: `.store()` or the field `.store`, with the method
    /// called on it where there is one (`.store().get_block`).
    Handle {
        /// The name of the accessor or the field.
        name: String,
        /// How the code reads, without the receiver and the arguments.
        written: String,
    },
}

/// A name a module gives to a path, with `use` or `type`: `fvoci_server::app::Database`
/// for `fvoci_store::Store`. Whoever names the first names the second.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Alias {
    pub(crate) name: String,
    pub(crate) target: String,
}

/// A module that a file declares and keeps in another file (`mod name;`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Declared {
    /// The module's path: `fvoci_server::api::contract`.
    pub(crate) module: Vec<String>,
    /// The inline modules between the file and the declaration, outermost first.
    pub(crate) inline_parents: Vec<String>,
    /// The value of `#[path = "…"]`, if the declaration has one.
    pub(crate) path_attribute: Option<String>,
}

/// What one file holds.
#[derive(Debug, Default)]
pub(crate) struct Source {
    pub(crate) references: Vec<Reference>,
    pub(crate) aliases: Vec<Alias>,
    pub(crate) declared: Vec<Declared>,
}

/// Reads the source of the module `module` (its path from the crate's name on).
///
/// Seen: every path in `use` (renamed, grouped, globbed), in expressions, types,
/// patterns, bounds and attributes, and what looks like a path of two names or more
/// among the tokens of a macro. `crate`, `self`, `super`, the modules declared in the
/// file and the names the file gives with `use` and `type` are resolved; an inline
/// module also sees those of the modules around it, as a test module does through
/// `use super::*`. A handle is seen as `.name()` without arguments or as the field
/// `.name`, anywhere in the file, macros included.
///
/// Not seen: an imported name used alone (`Store`, after `use fvoci_store::Store`: the
/// import counts, and every path of two names or more that starts at it); a type that
/// arrives without being named, as the result of a function or inside another type; a
/// handle kept in a variable and used from there, or reached under another name; code
/// that a macro writes or `include!` brings in.
///
/// # Errors
///
/// Fails if the source is not Rust that parses.
pub(crate) fn read_source(module: &[String], source: &str, handles: &[&str]) -> Result<Source> {
    let file = syn::parse_file(source)
        .map_err(|error| anyhow!("line {}: {error}", error.span().start().line))?;
    let mut reader = Reader::default();
    reader.read_items(module.to_vec(), &file.items, &file.attrs);
    let tokens: TokenStream = source
        .parse()
        .map_err(|error| anyhow!("the tokens do not read: {error}"))?;
    let mut read = reader.finish();
    read_handles(tokens, handles, &module.join("::"), &mut read.references);
    read.references.sort_by_key(|reference| reference.line);
    Ok(read)
}

/// Where a written path starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Start {
    /// At what is in sight where it is written.
    Scope,
    /// At the crates: `::name::…`.
    Crates,
}

impl Start {
    /// Where a path starts that syn has parsed.
    fn of(leading_colon: Option<&syn::token::PathSep>) -> Self {
        leading_colon.map_or(Self::Scope, |_| Self::Crates)
    }
}

/// A module whose items are being read, with the names of the modules declared in it.
struct Scope {
    module: Vec<String>,
    children: BTreeSet<String>,
}

/// A path as the file writes it, resolved as far as the modules tell.
struct Written {
    line: usize,
    /// The module of the code, then the modules around it in the file, outwards.
    modules: Vec<String>,
    /// The path: absolute, or as written where its first name is not known yet.
    names: Vec<String>,
    /// Whether the first name may be one the file gave with `use` or `type`.
    open: bool,
    /// The name this path is given, if it is the target of a `use` or part of a `type`.
    alias: Option<String>,
}

#[derive(Default)]
struct Reader {
    /// The module of the file, then the inline modules down to the code being read.
    scopes: Vec<Scope>,
    written: Vec<Written>,
    declared: Vec<Declared>,
    /// The name the paths being read are given: inside `type Name = …`.
    naming: Option<String>,
}

impl Reader {
    fn read_items(
        &mut self,
        module: Vec<String>,
        items: &[syn::Item],
        attributes: &[syn::Attribute],
    ) {
        let children = items
            .iter()
            .filter_map(|item| match item {
                syn::Item::Mod(module) => Some(module.ident.unraw().to_string()),
                _ => None,
            })
            .collect();
        self.scopes.push(Scope { module, children });
        for attribute in attributes {
            self.visit_attribute(attribute);
        }
        for item in items {
            self.visit_item(item);
        }
        self.scopes.pop();
    }

    /// The module of the code being read.
    fn module(&self) -> &[String] {
        self.scopes.last().map_or(&[], |scope| &scope.module)
    }

    /// Makes a path absolute as far as the modules tell: the path, and whether its
    /// first name is still open. `None` for a path that leads nowhere outside what it
    /// is written in (`Self::…`, more `super` than there are modules).
    fn resolve(&self, start: Start, names: &[String]) -> Option<(Vec<String>, bool)> {
        let (first, rest) = names.split_first()?;
        if start == Start::Crates {
            return Some((names.to_vec(), false));
        }
        let mut path = match first.as_str() {
            "Self" => return None,
            "crate" => vec![self.scopes.first()?.module.first()?.clone()],
            "self" => self.module().to_vec(),
            "super" => {
                let mut path = self.module().to_vec();
                path.pop()?;
                let mut rest = rest;
                while let Some((next, after)) = rest.split_first()
                    && next == "super"
                {
                    path.pop()?;
                    rest = after;
                }
                path.extend(rest.iter().cloned());
                return (!path.is_empty()).then_some((path, false));
            }
            // A module declared here, or around an inline module.
            _ => match self
                .scopes
                .iter()
                .rev()
                .find(|scope| scope.children.contains(first))
            {
                Some(scope) => {
                    let mut path = scope.module.clone();
                    path.push(first.clone());
                    path
                }
                None => return Some((names.to_vec(), true)),
            },
        };
        path.extend(rest.iter().cloned());
        Some((path, false))
    }

    /// Notes a path. `alias` is the name a `use` gives it.
    fn record(&mut self, start: Start, names: &[String], line: usize, alias: Option<&str>) {
        let Some((names, open)) = self.resolve(start, names) else {
            return;
        };
        let alias = alias
            .map(|name| format!("{}::{name}", self.module().join("::")))
            .or_else(|| self.naming.clone());
        self.written.push(Written {
            line,
            modules: self
                .scopes
                .iter()
                .rev()
                .map(|scope| scope.module.join("::"))
                .collect(),
            names,
            open,
            alias,
        });
    }

    fn read_use(&mut self, start: Start, prefix: &mut Vec<String>, tree: &syn::UseTree) {
        match tree {
            syn::UseTree::Path(path) => {
                prefix.push(path.ident.unraw().to_string());
                self.read_use(start, prefix, &path.tree);
                prefix.pop();
            }
            syn::UseTree::Name(syn::UseName { ident }) => {
                let line = ident.span().start().line;
                // `use a::{self}` names `a`, and gives it the name `a`.
                if ident == "self" && !prefix.is_empty() {
                    let alias = prefix.last().cloned();
                    self.record(start, prefix, line, alias.as_deref());
                } else {
                    let name = ident.unraw().to_string();
                    prefix.push(name.clone());
                    self.record(start, prefix, line, Some(&name));
                    prefix.pop();
                }
            }
            syn::UseTree::Rename(syn::UseRename { ident, rename, .. }) => {
                let alias = (rename != "_").then(|| rename.unraw().to_string());
                let line = ident.span().start().line;
                if ident == "self" && !prefix.is_empty() {
                    self.record(start, prefix, line, alias.as_deref());
                } else {
                    prefix.push(ident.unraw().to_string());
                    self.record(start, prefix, line, alias.as_deref());
                    prefix.pop();
                }
            }
            syn::UseTree::Glob(glob) => {
                prefix.push("*".to_owned());
                self.record(start, prefix, glob.star_token.span.start().line, None);
                prefix.pop();
            }
            syn::UseTree::Group(group) => {
                for tree in &group.items {
                    self.read_use(start, prefix, tree);
                }
            }
        }
    }

    /// Finds what reads like a path of two names or more among tokens that are not
    /// parsed: the input of a macro, the arguments of an attribute.
    fn read_tokens(&mut self, tokens: TokenStream) {
        let trees: Vec<TokenTree> = tokens.into_iter().collect();
        let mut at = 0;
        while let Some(tree) = trees.get(at) {
            match tree {
                TokenTree::Group(group) => {
                    self.read_tokens(group.stream());
                    at += 1;
                }
                TokenTree::Ident(first) => {
                    let mut names = vec![first.unraw().to_string()];
                    let mut end = at;
                    while is_path_separator(&trees, end + 1)
                        && let Some(TokenTree::Ident(next)) = trees.get(end + 3)
                    {
                        names.push(next.unraw().to_string());
                        end += 3;
                    }
                    let after_separator = at >= 2 && is_path_separator(&trees, at - 2);
                    // `<T as Trait>::name` and `Vec::<T>::new` go on from what is before.
                    let continues = after_separator
                        && matches!(at.checked_sub(3).and_then(|before| trees.get(before)),
                            Some(TokenTree::Punct(punct)) if punct.as_char() == '>');
                    let member = at >= 1 && is_punct(&trees, at - 1, '.');
                    let start = if after_separator {
                        Start::Crates
                    } else {
                        Start::Scope
                    };
                    if names.len() >= 2 && !continues && !member {
                        self.record(start, &names, first.span().start().line, None);
                    }
                    at = end + 1;
                }
                TokenTree::Punct(_) | TokenTree::Literal(_) => at += 1,
            }
        }
    }

    /// Resolves the first names that were left open against the names the file gives,
    /// now that all of them are known.
    fn finish(self) -> Source {
        let given: BTreeSet<&str> = self
            .written
            .iter()
            .filter_map(|written| written.alias.as_deref())
            .collect();
        let mut source = Source {
            declared: self.declared,
            ..Source::default()
        };
        for written in &self.written {
            let starts_at = written
                .names
                .first()
                .filter(|_| written.open)
                .and_then(|first| {
                    written
                        .modules
                        .iter()
                        .map(|module| format!("{module}::{first}"))
                        .find(|name| given.contains(name.as_str()))
                })
                // A name is not given to itself: `use a::b; use b::c;`.
                .filter(|name| written.alias.as_ref() != Some(name));
            let path = match &starts_at {
                Some(name) => {
                    let mut path = vec![name.clone()];
                    path.extend(written.names.iter().skip(1).cloned());
                    path.join("::")
                }
                None => written.names.join("::"),
            };
            if let Some(name) = &written.alias {
                source.aliases.push(Alias {
                    name: name.clone(),
                    target: path.clone(),
                });
            }
            source.references.push(Reference {
                line: written.line,
                module: written.modules.first().cloned().unwrap_or_default(),
                target: Target::Path(path),
            });
        }
        source
    }
}

impl<'ast> Visit<'ast> for Reader {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let mut module = self.module().to_vec();
        module.push(node.ident.unraw().to_string());
        if let Some((_, items)) = &node.content {
            self.read_items(module, items, &node.attrs);
            return;
        }
        for attribute in &node.attrs {
            self.visit_attribute(attribute);
        }
        let inline_parents = self
            .scopes
            .iter()
            .skip(1)
            .filter_map(|scope| scope.module.last().cloned())
            .collect();
        self.declared.push(Declared {
            module,
            inline_parents,
            path_attribute: node.attrs.iter().find_map(path_attribute),
        });
    }

    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        self.read_use(
            Start::of(node.leading_colon.as_ref()),
            &mut Vec::new(),
            &node.tree,
        );
    }

    fn visit_item_extern_crate(&mut self, node: &'ast syn::ItemExternCrate) {
        let name = node.ident.unraw().to_string();
        let alias = match &node.rename {
            Some((_, rename)) if rename != "_" => rename.unraw().to_string(),
            _ => name.clone(),
        };
        let line = node.ident.span().start().line;
        self.record(Start::Crates, &[name], line, Some(&alias));
    }

    /// `type Name = …` gives `Name` to every path of the type it stands for.
    fn visit_item_type(&mut self, node: &'ast syn::ItemType) {
        let name = format!("{}::{}", self.module().join("::"), node.ident.unraw());
        let outer = self.naming.replace(name);
        visit::visit_item_type(self, node);
        self.naming = outer;
    }

    fn visit_path(&mut self, node: &'ast syn::Path) {
        let names: Vec<String> = node
            .segments
            .iter()
            .map(|segment| segment.ident.unraw().to_string())
            .collect();
        // One name alone is a local or a type in sight; it counts where it is given
        // a name (`type Database = Store;`), as the given name may be used elsewhere.
        if let Some(first) = node.segments.first()
            && (names.len() >= 2 || self.naming.is_some())
        {
            let start = Start::of(node.leading_colon.as_ref());
            self.record(start, &names, first.ident.span().start().line, None);
        }
        visit::visit_path(self, node);
    }

    /// `pub(crate)` and `pub(in path)` say who may see an item, not what the item uses.
    fn visit_visibility(&mut self, _: &'ast syn::Visibility) {}

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        self.visit_path(&node.path);
        self.read_tokens(node.tokens.clone());
    }

    fn visit_meta_list(&mut self, node: &'ast syn::MetaList) {
        self.visit_path(&node.path);
        self.read_tokens(node.tokens.clone());
    }
}

/// The value of `#[path = "…"]`.
fn path_attribute(attribute: &syn::Attribute) -> Option<String> {
    if !attribute.path().is_ident("path") {
        return None;
    }
    match &attribute.meta {
        syn::Meta::NameValue(syn::MetaNameValue {
            value:
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(text),
                    ..
                }),
            ..
        }) => Some(text.value()),
        _ => None,
    }
}

fn is_punct(trees: &[TokenTree], at: usize, character: char) -> bool {
    matches!(trees.get(at), Some(TokenTree::Punct(punct)) if punct.as_char() == character)
}

/// Whether the two tokens from `at` are `::`.
fn is_path_separator(trees: &[TokenTree], at: usize) -> bool {
    matches!(trees.get(at), Some(TokenTree::Punct(punct))
        if punct.as_char() == ':' && punct.spacing() == Spacing::Joint)
        && is_punct(trees, at + 1, ':')
}

/// Finds `.name()` and `.name` for the names in `handles`, at any depth.
fn read_handles(tokens: TokenStream, handles: &[&str], module: &str, found: &mut Vec<Reference>) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    for (at, tree) in trees.iter().enumerate() {
        if let TokenTree::Group(group) = tree {
            read_handles(group.stream(), handles, module, found);
        }
        let TokenTree::Ident(ident) = tree else {
            continue;
        };
        let name = ident.unraw().to_string();
        // The dot of a member, not the second dot of a range (`a..name`).
        let member =
            at >= 1 && is_punct(&trees, at - 1, '.') && !(at >= 2 && is_punct(&trees, at - 2, '.'));
        if !member || !handles.contains(&name.as_str()) {
            continue;
        }
        let (mut written, next) = match trees.get(at + 1) {
            Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis => {
                // A method of that name that takes arguments is something else
                // (`AtomicBool::store`).
                if !group.stream().is_empty() {
                    continue;
                }
                (format!(".{name}()"), at + 2)
            }
            _ => (format!(".{name}"), at + 1),
        };
        if is_punct(&trees, next, '.')
            && let Some(TokenTree::Ident(method)) = trees.get(next + 1)
        {
            written.push('.');
            written.push_str(&method.unraw().to_string());
        }
        found.push(Reference {
            line: ident.span().start().line,
            module: module.to_owned(),
            target: Target::Handle { name, written },
        });
    }
}
