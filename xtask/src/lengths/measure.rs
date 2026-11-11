//! Measures one Rust source: the lines of the file and of every function in it.

use anyhow::{Result, anyhow};
use syn::ext::IdentExt as _;
use syn::visit::{self, Visit};

use super::{Length, Subject};

/// The length of the file `file` and of the functions written in it.
///
/// A function is measured from the line of its `fn` to the line of its closing brace,
/// with everything in between: comments, empty lines, closures, and the functions
/// inside it, which are measured on their own as well. Functions that a macro writes
/// are not seen.
///
/// # Errors
///
/// Fails if the source is not Rust that parses.
pub(crate) fn measure_source(file: &str, source: &str) -> Result<Vec<Length>> {
    let parsed = syn::parse_file(source)
        .map_err(|error| anyhow!("line {}: {error}", error.span().start().line))?;
    let mut meter = Meter {
        file,
        around: Vec::new(),
        measured: vec![Length {
            subject: Subject::File(file.to_owned()),
            lines: source.lines().count(),
            at: 1,
        }],
    };
    meter.visit_file(&parsed);
    Ok(meter.measured)
}

struct Meter<'a> {
    file: &'a str,
    /// The names around the code being read: inline modules, the type of an `impl`,
    /// a trait, functions.
    around: Vec<String>,
    measured: Vec<Length>,
}

impl Meter<'_> {
    fn measure(&mut self, signature: &syn::Signature, body: &syn::Block) {
        let first = signature.fn_token.span.start().line;
        let last = body.brace_token.span.close().start().line;
        let mut name = self.around.clone();
        name.push(signature.ident.unraw().to_string());
        self.measured.push(Length {
            subject: Subject::Function(self.file.to_owned(), name.join("::")),
            lines: last.saturating_sub(first) + 1,
            at: first,
        });
    }

    /// Reads what is inside something named `name`.
    fn inside(&mut self, name: String, read: impl FnOnce(&mut Self)) {
        self.around.push(name);
        read(self);
        self.around.pop();
    }
}

impl<'ast> Visit<'ast> for Meter<'_> {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        self.inside(node.ident.unraw().to_string(), |meter| {
            visit::visit_item_mod(meter, node);
        });
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        self.inside(type_name(&node.self_ty), |meter| {
            visit::visit_item_impl(meter, node);
        });
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        self.inside(node.ident.unraw().to_string(), |meter| {
            visit::visit_item_trait(meter, node);
        });
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        self.measure(&node.sig, &node.block);
        self.inside(node.sig.ident.unraw().to_string(), |meter| {
            visit::visit_item_fn(meter, node);
        });
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        self.measure(&node.sig, &node.block);
        self.inside(node.sig.ident.unraw().to_string(), |meter| {
            visit::visit_impl_item_fn(meter, node);
        });
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        if let Some(body) = &node.default {
            self.measure(&node.sig, body);
        }
        self.inside(node.sig.ident.unraw().to_string(), |meter| {
            visit::visit_trait_item_fn(meter, node);
        });
    }
}

/// The name of the type an `impl` is for, without its path and its arguments; `_` for a
/// type that has none (a tuple, a slice).
fn type_name(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(path) => path.path.segments.last().map_or_else(
            || "_".to_owned(),
            |segment| segment.ident.unraw().to_string(),
        ),
        syn::Type::Reference(reference) => type_name(&reference.elem),
        syn::Type::Paren(inner) => type_name(&inner.elem),
        syn::Type::Group(inner) => type_name(&inner.elem),
        _ => "_".to_owned(),
    }
}
