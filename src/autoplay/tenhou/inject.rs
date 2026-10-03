//! Opening a door into the Tenhou client's closed scope.
//!
//! The client is one immediately-invoked function expression — everything it
//! declares lives in that call and nothing reaches `window` (it exports only
//! `AudioContext`, `onerror` and `requestAnimationFrame`). JavaScript offers
//! no way to enumerate a closed function scope, so from an injected script its
//! handlers simply do not exist.
//!
//! That is the whole reason the discard was being done by computing a pixel
//! position and synthesising a click. The client has an entry point that needs
//! neither:
//!
//! ```js
//! "c21", function(a){ ...; var n={tag:"D",p:a}; 1==U.a&&Yb.S(n); n[1]=a; Nb.cb(n) }
//! ```
//!
//! `c21` takes the *tile index* (`0..=135`) — not a slot, not a coordinate —
//! sends the frame and applies the discard to its own board. Reaching it
//! removes the entire coordinate problem: no canvas geometry, no tile pitch,
//! no drawn-tile gap, and no exposure to the hand shifting when a call is
//! made.
//!
//! So the client script is rewritten in flight, before it runs, to publish the
//! handler registry on `window`.
//!
//! # Why the name is derived, not hard-coded
//!
//! The registry is called `R` in the build this was read from, but that is a
//! minifier's choice and Tenhou reissues the script under a new version
//! whenever it changes. A hard-coded `R` would keep matching *something* after
//! such an update — the wrong thing — so the name is instead recovered from a
//! shape the minifier cannot rename: the string literal `"c21"` under which
//! the handler registers itself.

use std::fmt;

/// Global the rewritten script publishes the registry on.
pub const EXPORT_GLOBAL: &str = "__akagiTenhou";

/// Marker the rewrite leaves behind so a second pass is a no-op.
const REWRITE_MARKER: &str = "/*akagi-injected*/";

/// How far back from the `"c21"` registration to look for its container.
const LOOKBEHIND: usize = 4096;

#[derive(Debug, PartialEq, Eq)]
pub enum InjectError {
    /// The script does not register a `c21` handler. Either it is not the
    /// Tenhou client, or the client changed how it registers handlers.
    NoDiscardHandler,
    /// Found the handler but not the object it registers into.
    NoRegistry,
    /// Derived a name that the script never calls back, so it is not the
    /// registry — refuse rather than publish something arbitrary.
    RegistryUnconfirmed(String),
    /// No place to append the export.
    NoInjectionPoint,
}

impl fmt::Display for InjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDiscardHandler => write!(f, "no `\"c21\"` handler registration found"),
            Self::NoRegistry => write!(f, "found the c21 handler but not its registry object"),
            Self::RegistryUnconfirmed(n) => {
                write!(f, "candidate registry `{n}` is never called as `{n}.c21(`")
            }
            Self::NoInjectionPoint => write!(f, "script does not end in an IIFE call"),
        }
    }
}

/// Is `b` valid inside a JavaScript identifier?
fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

/// Recover the name of the object the client registers its handlers into.
///
/// Anchored on `"c21"`, a string literal the minifier must preserve because
/// the client looks handlers up by name at runtime (its DOM dispatcher splits
/// an element's `name` attribute and indexes the registry with it). From that
/// anchor, walk back to the nearest `qa(<name>,[` — the registration helper —
/// and then confirm the name by requiring the script to call `<name>.c21(`
/// somewhere, which the discard path does. Without that confirmation a
/// coincidental match could publish an unrelated object.
pub fn derive_registry_name(src: &str) -> Result<&str, InjectError> {
    let bytes = src.as_bytes();
    let anchor = src
        .find("\"c21\",function(")
        .ok_or(InjectError::NoDiscardHandler)?;

    let window_start = anchor.saturating_sub(LOOKBEHIND);
    let call = src[window_start..anchor]
        .rfind("qa(")
        .map(|i| window_start + i + 3)
        .ok_or(InjectError::NoRegistry)?;

    let mut end = call;
    while end < bytes.len() && is_ident_byte(bytes[end]) {
        end += 1;
    }
    if end == call || bytes.get(end) != Some(&b',') {
        return Err(InjectError::NoRegistry);
    }
    let name = &src[call..end];

    // The discard path calls the registry back by name; a candidate that is
    // never used that way is not it.
    if !src.contains(&format!("{name}.c21(")) {
        return Err(InjectError::RegistryUnconfirmed(name.to_string()));
    }
    Ok(name)
}

/// Rewrite the client script so its handler registry is reachable.
///
/// The export is appended *inside* the outermost IIFE — immediately before its
/// final `})();` — because that is the only place the registry is still in
/// scope. It is wrapped in `try`/`catch` so a failure here can never stop the
/// client from starting: the worst case is that autoplay finds no door and
/// says so, not a page that will not load.
pub fn rewrite_client(src: &str) -> Result<String, InjectError> {
    if src.contains(REWRITE_MARKER) {
        return Ok(src.to_string());
    }
    let name = derive_registry_name(src)?;
    let point = src.rfind("})();").ok_or(InjectError::NoInjectionPoint)?;

    let export =
        format!(";{REWRITE_MARKER}try{{window.{EXPORT_GLOBAL}={{R:{name}}};}}catch(e){{}}\n");
    let mut out = String::with_capacity(src.len() + export.len());
    out.push_str(&src[..point]);
    out.push_str(&export);
    out.push_str(&src[point..]);
    Ok(out)
}

/// Expression that discards `tile_index` through the client's own handler.
pub fn discard_expression(tile_index: u32) -> String {
    format!(
        "(()=>{{const a=window.{EXPORT_GLOBAL};\
          if(!a||!a.R||typeof a.R.c21!=='function')return 'no-door';\
          a.R.c21({tile_index});return 'ok';}})()"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shaped like the real client: one IIFE, the registry built by `qa`, the
    /// handler registered under a string literal, and called back by name.
    fn sample(registry: &str) -> String {
        format!(
            "(function(){{var aa;var m=qa({{}},[1,2]);\
             var {registry}=qa({{}},[\"c0\",function(){{}}]);\
             qa({registry},[\"c4\",function(a){{var n=V[116][0];{registry}.c21(n[a].v)}},\
             \"c21\",function(a){{var n={{tag:\"D\",p:a}};Yb.S(n);Nb.cb(n)}}]);\
             (function(){{start()}})();\n}})();\n//"
        )
    }

    #[test]
    fn derives_the_registry_name() {
        assert_eq!(derive_registry_name(&sample("R")), Ok("R"));
    }

    /// The point of deriving rather than hard-coding: a rebuild renames it.
    #[test]
    fn survives_the_minifier_renaming_it() {
        for name in ["R", "Zq", "$a", "_7", "Wb"] {
            assert_eq!(derive_registry_name(&sample(name)), Ok(name));
        }
    }

    /// A name that is never called back is not the registry. Publishing an
    /// arbitrary object would fail later and further away.
    #[test]
    fn refuses_a_candidate_the_script_never_calls_back() {
        let src = "(function(){var Q={};qa(Q,[\"c21\",function(a){}]);})();";
        assert_eq!(
            derive_registry_name(src),
            Err(InjectError::RegistryUnconfirmed("Q".into()))
        );
    }

    #[test]
    fn reports_a_script_that_is_not_the_client() {
        assert_eq!(
            derive_registry_name("console.log(1)"),
            Err(InjectError::NoDiscardHandler)
        );
    }

    /// The export has to land inside the outermost IIFE, where the registry
    /// is still in scope — appending after it would capture nothing.
    #[test]
    fn injects_before_the_last_iife_close() {
        let out = rewrite_client(&sample("R")).unwrap();
        let export = out.find(EXPORT_GLOBAL).unwrap();
        let close = out.rfind("})();").unwrap();
        assert!(export < close, "export must precede the IIFE close");
        assert!(out.contains("window.__akagiTenhou={R:R};"));
        assert!(out.ends_with("//"), "tail preserved");
    }

    /// Nothing in the injection may be able to stop the client booting.
    #[test]
    fn injection_cannot_throw_into_the_client() {
        let out = rewrite_client(&sample("R")).unwrap();
        let i = out.find(EXPORT_GLOBAL).unwrap();
        let stmt = &out[i.saturating_sub(20)..];
        assert!(stmt.starts_with("try{") || stmt.contains("try{"));
        assert!(out.contains("catch(e){}"));
    }

    #[test]
    fn rewriting_twice_changes_nothing() {
        let once = rewrite_client(&sample("R")).unwrap();
        let twice = rewrite_client(&once).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn a_foreign_script_is_refused_not_mangled() {
        assert!(rewrite_client("window.foo=1;").is_err());
    }

    /// The discard expression carries the tile index and reports a missing
    /// door instead of throwing.
    #[test]
    fn discard_expression_is_index_addressed() {
        let e = discard_expression(88);
        assert!(e.contains("c21(88)"));
        assert!(e.contains("no-door"));
    }
}
