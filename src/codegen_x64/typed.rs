//! The typed program: the declarations the checker settles before any code is emitted.
//!
//! Records, enums, interfaces, signatures, globals and the facts derived from them live here, apart
//! from the emission state of `Compiler` (labels, frame slots, assembly text). Code generation reads
//! this form for everything it asks about a declaration, and `load`/`declare` are what fill it.

use super::*;

#[derive(Default)]
pub(super) struct TypedProgram {
    pub(super) rec_decls: HashMap<String, (String, Vec<Field>)>,
    pub(super) records: HashMap<String, RecInfo>,
    pub(super) enum_decls: HashMap<String, (String, Vec<Variant>)>,
    pub(super) enums: HashMap<String, EnumInfo>,
    /// (module, variant name) -> (enum, index)
    pub(super) variants: HashMap<(String, String), (String, usize)>,
    pub(super) aliases: HashMap<String, (String, Type)>,
    /// `#[shared]` records have reference semantics: never copied on assignment, capture or iteration.
    pub(super) shared_recs: HashSet<String>,
    /// records that `new` allocates (they get a `B_rcdrop_*` wrapper); never rolled back
    pub(super) rc_recs: HashSet<String>,
    pub(super) enum_payload: HashSet<String>,
    /// qualified names of the types written with `pub`
    pub(super) pub_types: HashSet<String>,
    /// Interfaces: qualified name -> (module, method signatures).
    pub(super) interfaces: HashMap<String, (String, Vec<MethodSig>)>,
    pub(super) funcs: HashMap<(String, String), Sig>,
    pub(super) methods: HashMap<(String, String), Sig>,
    pub(super) globals: HashMap<(String, String), Global>,
    /// Methods the compiler wrote itself (derived `eq`, `compare`, `hash`): (type key, name).
    pub(super) derived: HashSet<(String, String)>,
    /// functions marked `#[export]`
    pub(super) exported: std::collections::HashSet<String>,
    // ---- generics: what the instantiations of the whole program decided ----
    /// Instance of a generic record -> (template, type arguments).
    pub(super) inst_args: HashMap<String, (String, Vec<Ty>)>,
    /// (module, function, type arguments) -> instance name.
    pub(super) inst_funcs: HashMap<(String, String, String), String>,
    /// Instances waiting to be compiled.
    pub(super) pending: Vec<Pending>,
    pub(super) inst_count: usize,
    /// Hash and equality thunks of general map keys, by key type: (hash symbol, equality symbol).
    pub(super) map_thunks: BTreeMap<String, (String, String)>,
}
