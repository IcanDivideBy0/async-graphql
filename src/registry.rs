use crate::parser::ast::Type as ParsedType;
use crate::validators::InputValueValidator;
use crate::{model, Any, GqlValue, Type as _};
use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::sync::Arc;

fn parse_non_null(type_name: &str) -> Option<&str> {
    if type_name.ends_with('!') {
        Some(&type_name[..type_name.len() - 1])
    } else {
        None
    }
}

fn parse_list(type_name: &str) -> Option<&str> {
    if type_name.starts_with('[') {
        Some(&type_name[1..type_name.len() - 1])
    } else {
        None
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TypeName<'a> {
    List(&'a str),
    NonNull(&'a str),
    Named(&'a str),
}

impl<'a> std::fmt::Display for TypeName<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeName::Named(name) => write!(f, "{}", name),
            TypeName::NonNull(name) => write!(f, "{}!", name),
            TypeName::List(name) => write!(f, "[{}]", name),
        }
    }
}

impl<'a> TypeName<'a> {
    pub fn create(type_name: &str) -> TypeName {
        if let Some(type_name) = parse_non_null(type_name) {
            TypeName::NonNull(type_name)
        } else if let Some(type_name) = parse_list(type_name) {
            TypeName::List(type_name)
        } else {
            TypeName::Named(type_name)
        }
    }

    pub fn concrete_typename(type_name: &str) -> &str {
        match TypeName::create(type_name) {
            TypeName::List(type_name) => Self::concrete_typename(type_name),
            TypeName::NonNull(type_name) => Self::concrete_typename(type_name),
            TypeName::Named(type_name) => type_name,
        }
    }

    pub fn is_non_null(&self) -> bool {
        if let TypeName::NonNull(_) = self {
            true
        } else {
            false
        }
    }

    pub fn unwrap_non_null(&self) -> Self {
        match self {
            TypeName::NonNull(ty) => TypeName::create(ty),
            _ => *self,
        }
    }

    pub fn is_subtype(&self, sub: &TypeName<'_>) -> bool {
        match (self, sub) {
            (TypeName::NonNull(super_type), TypeName::NonNull(sub_type))
            | (TypeName::Named(super_type), TypeName::NonNull(sub_type)) => {
                TypeName::create(super_type).is_subtype(&TypeName::create(sub_type))
            }
            (TypeName::Named(super_type), TypeName::Named(sub_type)) => super_type == sub_type,
            (TypeName::List(super_type), TypeName::List(sub_type)) => {
                TypeName::create(super_type).is_subtype(&TypeName::create(sub_type))
            }
            _ => false,
        }
    }
}

#[derive(Clone)]
pub struct InputValue {
    pub name: &'static str,
    pub description: Option<&'static str>,
    pub ty: String,
    pub default_value: Option<&'static str>,
    pub validator: Option<Arc<dyn InputValueValidator>>,
}

#[derive(Clone)]
pub struct Field {
    pub name: String,
    pub description: Option<&'static str>,
    pub args: HashMap<&'static str, InputValue>,
    pub ty: String,
    pub deprecation: Option<&'static str>,
    pub cache_control: CacheControl,
    pub external: bool,
    pub requires: Option<&'static str>,
    pub provides: Option<&'static str>,
}

#[derive(Clone)]
pub struct EnumValue {
    pub name: &'static str,
    pub description: Option<&'static str>,
    pub deprecation: Option<&'static str>,
}

/// Cache control values
///
/// # Examples
///
/// ```rust
/// use async_graphql::prelude::*;
/// use async_graphql::{EmptyMutation, EmptySubscription, CacheControl};
///
/// struct QueryRoot;
///
/// #[GqlObject(cache_control(max_age = 60))]
/// impl QueryRoot {
///     #[field(cache_control(max_age = 30))]
///     async fn value1(&self) -> i32 {
///         0
///     }
///
///     #[field(cache_control(private))]
///     async fn value2(&self) -> i32 {
///         0
///     }
/// }
///
/// #[async_std::main]
/// async fn main() {
///     let schema = GqlSchema::new(QueryRoot, EmptyMutation, EmptySubscription);
///     assert_eq!(schema.execute("{ value1 }").await.unwrap().cache_control, CacheControl { public: true, max_age: 30 });
///     assert_eq!(schema.execute("{ value2 }").await.unwrap().cache_control, CacheControl { public: false, max_age: 60 });
///     assert_eq!(schema.execute("{ value1 value2 }").await.unwrap().cache_control, CacheControl { public: false, max_age: 30 });
/// }
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CacheControl {
    /// Scope is public, default is true
    pub public: bool,

    /// Cache max age, default is 0.
    pub max_age: usize,
}

impl Default for CacheControl {
    fn default() -> Self {
        Self {
            public: true,
            max_age: 0,
        }
    }
}

impl CacheControl {
    /// Get 'Cache-Control' header value.
    pub fn value(&self) -> Option<String> {
        if self.max_age > 0 {
            if !self.public {
                Some(format!("max-age={}, private", self.max_age))
            } else {
                Some(format!("max-age={}", self.max_age))
            }
        } else {
            None
        }
    }
}

impl CacheControl {
    pub(crate) fn merge(&mut self, other: &CacheControl) {
        self.public = self.public && other.public;
        self.max_age = if self.max_age == 0 {
            other.max_age
        } else if other.max_age == 0 {
            self.max_age
        } else {
            self.max_age.min(other.max_age)
        };
    }
}

pub enum Type {
    Scalar {
        name: String,
        description: Option<&'static str>,
        is_valid: fn(value: &GqlValue) -> bool,
    },
    Object {
        name: String,
        description: Option<&'static str>,
        fields: HashMap<String, Field>,
        cache_control: CacheControl,
        extends: bool,
        keys: Option<Vec<String>>,
    },
    Interface {
        name: String,
        description: Option<&'static str>,
        fields: HashMap<String, Field>,
        possible_types: HashSet<String>,
        extends: bool,
        keys: Option<Vec<String>>,
    },
    Union {
        name: String,
        description: Option<&'static str>,
        possible_types: HashSet<String>,
    },
    Enum {
        name: String,
        description: Option<&'static str>,
        enum_values: HashMap<&'static str, EnumValue>,
    },
    InputObject {
        name: String,
        description: Option<&'static str>,
        input_fields: HashMap<String, InputValue>,
    },
}

impl Type {
    pub fn field_by_name(&self, name: &str) -> Option<&Field> {
        self.fields().and_then(|fields| fields.get(name))
    }

    pub fn fields(&self) -> Option<&HashMap<String, Field>> {
        match self {
            Type::Object { fields, .. } => Some(&fields),
            Type::Interface { fields, .. } => Some(&fields),
            _ => None,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Type::Scalar { name, .. } => &name,
            Type::Object { name, .. } => name,
            Type::Interface { name, .. } => name,
            Type::Union { name, .. } => name,
            Type::Enum { name, .. } => name,
            Type::InputObject { name, .. } => name,
        }
    }

    pub fn is_composite(&self) -> bool {
        match self {
            Type::Object { .. } => true,
            Type::Interface { .. } => true,
            Type::Union { .. } => true,
            _ => false,
        }
    }

    pub fn is_abstract(&self) -> bool {
        match self {
            Type::Interface { .. } => true,
            Type::Union { .. } => true,
            _ => false,
        }
    }

    pub fn is_leaf(&self) -> bool {
        match self {
            Type::Enum { .. } => true,
            Type::Scalar { .. } => true,
            _ => false,
        }
    }

    pub fn is_input(&self) -> bool {
        match self {
            Type::Enum { .. } => true,
            Type::Scalar { .. } => true,
            Type::InputObject { .. } => true,
            _ => false,
        }
    }

    pub fn is_possible_type(&self, type_name: &str) -> bool {
        match self {
            Type::Interface { possible_types, .. } => possible_types.contains(type_name),
            Type::Union { possible_types, .. } => possible_types.contains(type_name),
            Type::Object { name, .. } => name == type_name,
            _ => false,
        }
    }

    pub fn possible_types(&self) -> Option<&HashSet<String>> {
        match self {
            Type::Interface { possible_types, .. } => Some(possible_types),
            Type::Union { possible_types, .. } => Some(possible_types),
            _ => None,
        }
    }

    pub fn type_overlap(&self, ty: &Type) -> bool {
        if self as *const Type == ty as *const Type {
            return true;
        }

        match (self.is_abstract(), ty.is_abstract()) {
            (true, true) => self
                .possible_types()
                .iter()
                .copied()
                .flatten()
                .any(|type_name| ty.is_possible_type(type_name)),
            (true, false) => self.is_possible_type(ty.name()),
            (false, true) => ty.is_possible_type(self.name()),
            (false, false) => false,
        }
    }
}

pub struct Directive {
    pub name: &'static str,
    pub description: Option<&'static str>,
    pub locations: Vec<model::__DirectiveLocation>,
    pub args: HashMap<&'static str, InputValue>,
}

pub struct Registry {
    pub types: HashMap<String, Type>,
    pub directives: HashMap<String, Directive>,
    pub implements: HashMap<String, HashSet<String>>,
    pub query_type: String,
    pub mutation_type: Option<String>,
    pub subscription_type: Option<String>,
}

impl Registry {
    pub fn create_type<T: crate::Type, F: FnMut(&mut Registry) -> Type>(
        &mut self,
        mut f: F,
    ) -> String {
        let name = T::type_name();
        if !self.types.contains_key(name.as_ref()) {
            self.types.insert(
                name.to_string(),
                Type::Object {
                    name: "".to_string(),
                    description: None,
                    fields: Default::default(),
                    cache_control: Default::default(),
                    extends: false,
                    keys: None,
                },
            );
            let ty = f(self);
            self.types.insert(name.to_string(), ty);
        }
        T::qualified_type_name()
    }

    pub fn add_directive(&mut self, directive: Directive) {
        self.directives
            .insert(directive.name.to_string(), directive);
    }

    pub fn add_implements(&mut self, ty: &str, interface: &str) {
        self.implements
            .entry(ty.to_string())
            .and_modify(|interfaces| {
                interfaces.insert(interface.to_string());
            })
            .or_insert({
                let mut interfaces = HashSet::new();
                interfaces.insert(interface.to_string());
                interfaces
            });
    }

    pub fn add_keys(&mut self, ty: &str, keys: &str) {
        let all_keys = match self.types.get_mut(ty) {
            Some(Type::Object { keys: all_keys, .. }) => all_keys,
            Some(Type::Interface { keys: all_keys, .. }) => all_keys,
            _ => return,
        };
        if let Some(all_keys) = all_keys {
            all_keys.push(keys.to_string());
        } else {
            *all_keys = Some(vec![keys.to_string()]);
        }
    }

    pub fn concrete_type_by_name(&self, type_name: &str) -> Option<&Type> {
        self.types.get(TypeName::concrete_typename(type_name))
    }

    pub fn concrete_type_by_parsed_type(&self, query_type: &ParsedType) -> Option<&Type> {
        match query_type {
            ParsedType::NonNull(ty) => self.concrete_type_by_parsed_type(ty),
            ParsedType::List(ty) => self.concrete_type_by_parsed_type(ty),
            ParsedType::Named(name) => self.types.get(name.as_str()),
        }
    }

    fn create_federation_fields<'a, I: Iterator<Item = &'a Field>>(sdl: &mut String, it: I) {
        for field in it {
            if field.name.starts_with("__") {
                continue;
            }
            if field.name == "_service" || field.name == "_entities" {
                continue;
            }

            write!(sdl, "\t{}: {}", field.name, field.ty).ok();
            if field.external {
                write!(sdl, " @external").ok();
            }
            if let Some(requires) = field.requires {
                write!(sdl, " @requires(fields: \"{}\")", requires).ok();
            }
            if let Some(provides) = field.provides {
                write!(sdl, " @provides(fields: \"{}\")", provides).ok();
            }
            writeln!(sdl).ok();
        }
    }

    fn create_federation_type(&self, ty: &Type, sdl: &mut String) {
        match ty {
            Type::Object {
                name,
                fields,
                extends,
                keys,
                ..
            } => {
                if name.starts_with("__") {
                    return;
                }
                if name == "_Service" {
                    return;
                }
                if fields.len() == 4 {
                    // Is empty query root, only __schema, __type, _service, _entities fields
                    return;
                }

                if *extends {
                    write!(sdl, "extend ").ok();
                }
                write!(sdl, "type {} ", name).ok();
                if let Some(keys) = keys {
                    for key in keys {
                        write!(sdl, "@key(fields: \"{}\") ", key).ok();
                    }
                }
                writeln!(sdl, "{{").ok();
                Self::create_federation_fields(sdl, fields.values());
                writeln!(sdl, "}}").ok();
            }
            Type::Interface {
                name,
                fields,
                extends,
                keys,
                ..
            } => {
                if *extends {
                    write!(sdl, "extend ").ok();
                }
                write!(sdl, "interface {} ", name).ok();
                if let Some(keys) = keys {
                    for key in keys {
                        write!(sdl, "@key(fields: \"{}\") ", key).ok();
                    }
                }
                writeln!(sdl, "{{").ok();
                Self::create_federation_fields(sdl, fields.values());
                writeln!(sdl, "}}").ok();
            }
            _ => {}
        }
    }

    pub fn create_federation_sdl(&self) -> String {
        let mut sdl = String::new();
        for ty in self.types.values() {
            self.create_federation_type(ty, &mut sdl);
        }
        sdl
    }

    fn has_entities(&self) -> bool {
        self.types.values().any(|ty| match ty {
            Type::Object {
                keys: Some(keys), ..
            } => !keys.is_empty(),
            Type::Interface {
                keys: Some(keys), ..
            } => !keys.is_empty(),
            _ => false,
        })
    }

    fn create_entity_type(&mut self) {
        let possible_types = self
            .types
            .values()
            .filter_map(|ty| match ty {
                Type::Object {
                    name,
                    keys: Some(keys),
                    ..
                } if !keys.is_empty() => Some(name.clone()),
                Type::Interface {
                    name,
                    keys: Some(keys),
                    ..
                } if !keys.is_empty() => Some(name.clone()),
                _ => None,
            })
            .collect();

        self.types.insert(
            "_Entity".to_string(),
            Type::Union {
                name: "_Entity".to_string(),
                description: None,
                possible_types,
            },
        );
    }

    pub fn create_federation_types(&mut self) {
        if !self.has_entities() {
            return;
        }

        Any::create_type_info(self);

        self.types.insert(
            "_Service".to_string(),
            Type::Object {
                name: "_Service".to_string(),
                description: None,
                fields: {
                    let mut fields = HashMap::new();
                    fields.insert(
                        "sdl".to_string(),
                        Field {
                            name: "sdl".to_string(),
                            description: None,
                            args: Default::default(),
                            ty: "String".to_string(),
                            deprecation: None,
                            cache_control: Default::default(),
                            external: false,
                            requires: None,
                            provides: None,
                        },
                    );
                    fields
                },
                cache_control: Default::default(),
                extends: false,
                keys: None,
            },
        );

        self.create_entity_type();

        let query_root = self.types.get_mut(&self.query_type).unwrap();
        if let Type::Object { fields, .. } = query_root {
            fields.insert(
                "_service".to_string(),
                Field {
                    name: "_service".to_string(),
                    description: None,
                    args: Default::default(),
                    ty: "_Service!".to_string(),
                    deprecation: None,
                    cache_control: Default::default(),
                    external: false,
                    requires: None,
                    provides: None,
                },
            );

            fields.insert(
                "_entities".to_string(),
                Field {
                    name: "_entities".to_string(),
                    description: None,
                    args: {
                        let mut args = HashMap::new();
                        args.insert(
                            "representations",
                            InputValue {
                                name: "representations",
                                description: None,
                                ty: "[_Any!]!".to_string(),
                                default_value: None,
                                validator: None,
                            },
                        );
                        args
                    },
                    ty: "[_Entity]!".to_string(),
                    deprecation: None,
                    cache_control: Default::default(),
                    external: false,
                    requires: None,
                    provides: None,
                },
            );
        }
    }
}
