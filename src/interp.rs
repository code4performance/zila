//! Tree-walking AST interpreter for Brasa.

use crate::ast::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// An insertion-ordered map. Keys are `text` or integers.
#[derive(Clone, Debug, Default)]
pub struct MapVal {
    entries: Vec<Option<(Value, Value)>>,
    index: HashMap<MapKey, usize>,
    live: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum MapKey {
    Int(i64),
    Text(String),
}

impl MapVal {
    fn key_of(k: &Value) -> Result<MapKey, String> {
        match k {
            Value::Int(n) => Ok(MapKey::Int(*n)),
            Value::Text(t) => Ok(MapKey::Text(t.clone())),
            other => Err(format!("{} cannot be a map key; keys are text or integers", other.type_name())),
        }
    }

    pub fn len(&self) -> usize {
        self.live
    }

    pub fn get(&self, k: &Value) -> Result<Option<Value>, String> {
        Ok(self.index.get(&Self::key_of(k)?).and_then(|i| self.entries[*i].as_ref()).map(|e| e.1.clone()))
    }

    pub fn set(&mut self, k: Value, v: Value) -> Result<(), String> {
        let key = Self::key_of(&k)?;
        if let Some(i) = self.index.get(&key) {
            self.entries[*i] = Some((k, v));
        } else {
            self.index.insert(key, self.entries.len());
            self.entries.push(Some((k, v)));
            self.live += 1;
        }
        Ok(())
    }

    pub fn remove(&mut self, k: &Value) -> Result<(), String> {
        if let Some(i) = self.index.remove(&Self::key_of(k)?) {
            self.entries[i] = None;
            self.live -= 1;
        }
        Ok(())
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.index.clear();
        self.live = 0;
    }

    /// The live entries in insertion order.
    pub fn pairs(&self) -> Vec<(Value, Value)> {
        self.entries.iter().flatten().cloned().collect()
    }
}

#[derive(Debug, Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Char(char),
    Text(String),
    Range(i64, i64),
    List(Rc<RefCell<Vec<Value>>>),
    Map(Rc<RefCell<MapVal>>),
    Record {
        type_name: String,
        fields: Rc<RefCell<HashMap<String, Value>>>,
    },
    Func(FuncDecl),
    Builtin(String),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Nil, Value::Nil) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Char(a), Value::Char(b)) => a == b,
            (Value::Text(a), Value::Text(b)) => a == b,
            (Value::Range(a1, a2), Value::Range(b1, b2)) => a1 == b1 && a2 == b2,
            _ => false,
        }
    }
}

impl Value {
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Nil => false,
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::Float(f) => *f != 0.0,
            Value::Text(s) => !s.is_empty(),
            _ => true,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Nil => "nil",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Char(_) => "char",
            Value::Text(_) => "text",
            Value::Range(_, _) => "range",
            Value::List(_) => "list",
            Value::Map(_) => "map",
            Value::Record { .. } => "record",
            Value::Func(_) => "func",
            Value::Builtin(_) => "builtin",
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Nil => write!(f, "nil"),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Int(n) => write!(f, "{}", n),
            Value::Float(n) => write!(f, "{}", float_text(*n)),
            Value::Char(c) => write!(f, "{}", c),
            Value::Text(s) => write!(f, "{}", s),
            Value::Range(start, end) => write!(f, "{}..{}", start, end),
            Value::List(l) => {
                let items: Vec<String> = l.borrow().iter().map(|v| v.to_string()).collect();
                write!(f, "[{}]", items.join(", "))
            }
            Value::Map(m) => {
                let items: Vec<String> = m.borrow().pairs().iter().map(|(k, v)| format!("{}: {}", k, v)).collect();
                write!(f, "{{{}}}", items.join(", "))
            }
            Value::Record { type_name, fields } => {
                let items: Vec<String> = fields
                    .borrow()
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v))
                    .collect();
                write!(f, "{} {{ {} }}", type_name, items.join(", "))
            }
            Value::Func(fd) => write!(f, "<func {}>", fd.name),
            Value::Builtin(name) => write!(f, "<builtin {}>", name),
        }
    }
}

#[derive(Clone)]
pub struct Env {
    vars: HashMap<String, Value>,
    parent: Option<Rc<RefCell<Env>>>,
}

impl Env {
    pub fn new() -> Self {
        Env {
            vars: HashMap::new(),
            parent: None,
        }
    }

    pub fn with_parent(parent: Rc<RefCell<Env>>) -> Self {
        Env {
            vars: HashMap::new(),
            parent: Some(parent),
        }
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        if let Some(v) = self.vars.get(name) {
            Some(v.clone())
        } else if let Some(parent) = &self.parent {
            parent.borrow().get(name)
        } else {
            None
        }
    }

    pub fn set(&mut self, name: &str, val: Value) -> bool {
        if self.vars.contains_key(name) {
            self.vars.insert(name.to_string(), val);
            true
        } else if let Some(parent) = &self.parent {
            parent.borrow_mut().set(name, val)
        } else {
            false
        }
    }

    pub fn define(&mut self, name: String, val: Value) {
        self.vars.insert(name, val);
    }
}

pub enum Flow {
    None,
    Return(Value),
    Break,
    Continue,
}

pub type InterpResult<T> = Result<T, String>;

pub struct Interpreter {
    pub globals: Rc<RefCell<Env>>,
    pub funcs: HashMap<String, FuncDecl>,
    pub methods: HashMap<(String, String), FuncDecl>,
    pub types: HashMap<String, TypeDecl>,
    /// One frame per running function: the `defer`red calls, with the environment they were deferred in.
    defers: Vec<Vec<(Expr, Rc<RefCell<Env>>)>>,
}

impl Interpreter {
    pub fn new() -> Self {
        let globals = Rc::new(RefCell::new(Env::new()));
        let mut interp = Interpreter {
            globals,
            funcs: HashMap::new(),
            methods: HashMap::new(),
            types: HashMap::new(),
            defers: Vec::new(),
        };
        interp.register_builtins();
        interp
    }

    fn register_builtins(&mut self) {
        let mut g = self.globals.borrow_mut();
        g.define("print".to_string(), Value::Builtin("print".to_string()));
        g.define("println".to_string(), Value::Builtin("println".to_string()));
        g.define("len".to_string(), Value::Builtin("len".to_string()));
        for n in ["float", "f64", "f32", "int", "i64"] {
            g.define(n.to_string(), Value::Builtin(n.to_string()));
        }
    }

    pub fn load_file(&mut self, file: &File) -> InterpResult<()> {
        let globals_ref = self.globals.clone();
        for decl in &file.decls {
            match &decl.kind {
                DeclKind::Func(fd) => {
                    if let Some(rec) = &fd.receiver {
                        let type_name = match &rec.ty {
                            Type::Named { path, .. } => path.last().cloned().unwrap_or_default(),
                            _ => "".to_string(),
                        };
                        self.methods.insert((type_name, fd.name.clone()), fd.clone());
                    } else {
                        self.funcs.insert(fd.name.clone(), fd.clone());
                    }
                }
                DeclKind::Type(td) => {
                    self.types.insert(td.name.clone(), td.clone());
                }
                DeclKind::Var(vd) => {
                    let val = if let Some(init) = &vd.value {
                        self.eval_expr(init, &globals_ref)?
                    } else {
                        Value::Nil
                    };
                    self.globals.borrow_mut().define(vd.name.clone(), val);
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub fn run_main(&mut self) -> InterpResult<Value> {
        let main_func = self
            .funcs
            .get("main")
            .cloned()
            .ok_or_else(|| "no 'main' function found".to_string())?;

        let env = Rc::new(RefCell::new(Env::with_parent(self.globals.clone())));
        match self.run_body(&main_func.body, &env)? {
            Flow::Return(v) => Ok(v),
            _ => Ok(Value::Nil),
        }
    }

    /// Runs a function body. Calls deferred in it run when it ends, last in first out.
    fn run_body(&mut self, body: &[Stmt], env: &Rc<RefCell<Env>>) -> InterpResult<Flow> {
        self.defers.push(Vec::new());
        let flow = self.eval_block(body, env);
        let pending = self.defers.pop().unwrap_or_default();
        let flow = flow?;
        for (expr, denv) in pending.into_iter().rev() {
            self.eval_expr(&expr, &denv)?;
        }
        Ok(flow)
    }

    pub fn eval_block(&mut self, block: &[Stmt], env: &Rc<RefCell<Env>>) -> InterpResult<Flow> {
        for stmt in block {
            let flow = self.eval_stmt(stmt, env)?;
            match flow {
                Flow::None => continue,
                other => return Ok(other),
            }
        }
        Ok(Flow::None)
    }

    pub fn eval_stmt(&mut self, stmt: &Stmt, env: &Rc<RefCell<Env>>) -> InterpResult<Flow> {
        match &stmt.kind {
            StmtKind::Var(vd) => {
                let val = if let Some(e) = &vd.value {
                    self.eval_expr(e, env)?
                } else {
                    Value::Nil
                };
                env.borrow_mut().define(vd.name.clone(), val);
                Ok(Flow::None)
            }
            StmtKind::Assign { target, op, value } => {
                let rhs = self.eval_expr(value, env)?;
                match target.kind {
                    ExprKind::Ident(ref name) => {
                        let current = env.borrow().get(name).unwrap_or(Value::Nil);
                        let final_val = self.apply_assign_op(op, current, rhs.clone())?;
                        if !env.borrow_mut().set(name, final_val) {
                            env.borrow_mut().define(name.clone(), rhs);
                        }
                    }
                    ExprKind::Field { ref expr, ref name, .. } => {
                        let rec = self.eval_expr(expr, env)?;
                        if let Value::Record { fields, .. } = rec {
                            let current = fields.borrow().get(name).cloned().unwrap_or(Value::Nil);
                            let final_val = self.apply_assign_op(op, current, rhs)?;
                            fields.borrow_mut().insert(name.clone(), final_val);
                        } else {
                            return Err(format!("line {}: field assignment on non-record", stmt.line));
                        }
                    }
                    ExprKind::Index { ref expr, ref index } => {
                        let target_val = self.eval_expr(expr, env)?;
                        let idx_val = self.eval_expr(index, env)?;
                        match target_val {
                            Value::List(l) => {
                                if let Value::Int(i) = idx_val {
                                    let mut list = l.borrow_mut();
                                    let idx = i as usize;
                                    if idx < list.len() {
                                        let current = list[idx].clone();
                                        list[idx] = self.apply_assign_op(op, current, rhs)?;
                                    } else {
                                        return Err(format!("line {}: index out of bounds", stmt.line));
                                    }
                                }
                            }
                            Value::Map(m) => {
                                let current = if *op == "=" {
                                    Value::Nil
                                } else {
                                    let found = m.borrow().get(&idx_val).map_err(|e| format!("line {}: {}", stmt.line, e))?;
                                    found.ok_or_else(|| format!("line {}: map key not found", stmt.line))?
                                };
                                let final_val = self.apply_assign_op(op, current, rhs)?;
                                m.borrow_mut().set(idx_val, final_val).map_err(|e| format!("line {}: {}", stmt.line, e))?;
                            }
                            _ => return Err(format!("line {}: indexing non-indexable value", stmt.line)),
                        }
                    }
                    _ => return Err(format!("line {}: invalid assignment target", stmt.line)),
                }
                Ok(Flow::None)
            }
            StmtKind::Return(opt_expr) => {
                let val = if let Some(e) = opt_expr {
                    self.eval_expr(e, env)?
                } else {
                    Value::Nil
                };
                Ok(Flow::Return(val))
            }
            StmtKind::Break => Ok(Flow::Break),
            StmtKind::Continue => Ok(Flow::Continue),
            StmtKind::Defer(expr) => {
                if !matches!(expr.kind, ExprKind::Call { .. }) {
                    return Err(format!("line {}: `defer` needs a call, like `defer file.close()`", stmt.line));
                }
                if let Some(frame) = self.defers.last_mut() {
                    frame.push((expr.clone(), env.clone()));
                }
                Ok(Flow::None)
            }
            StmtKind::Spawn(call) => {
                self.eval_expr(call, env)?;
                Ok(Flow::None)
            }
            StmtKind::Unsafe(body) => self.eval_block(body, env),
            StmtKind::Asm(_) => {
                Ok(Flow::None)
            }
            StmtKind::While { cond, body } => {
                while self.eval_expr(cond, env)?.is_truthy() {
                    let loop_env = Rc::new(RefCell::new(Env::with_parent(env.clone())));
                    match self.eval_block(body, &loop_env)? {
                        Flow::Break => break,
                        Flow::Continue => continue,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        Flow::None => {}
                    }
                }
                Ok(Flow::None)
            }
            StmtKind::For { vars, iter, body } => {
                let iter_val = self.eval_expr(iter, env)?;
                match iter_val {
                    Value::Range(start, end) => {
                        let var_name = vars.first().cloned().unwrap_or_else(|| "_".to_string());
                        for i in start..end {
                            let loop_env = Rc::new(RefCell::new(Env::with_parent(env.clone())));
                            loop_env.borrow_mut().define(var_name.clone(), Value::Int(i));
                            match self.eval_block(body, &loop_env)? {
                                Flow::Break => break,
                                Flow::Continue => continue,
                                Flow::Return(v) => return Ok(Flow::Return(v)),
                                Flow::None => {}
                            }
                        }
                    }
                    Value::Map(m) => {
                        let pairs = m.borrow().pairs();
                        for (k, v) in pairs {
                            let loop_env = Rc::new(RefCell::new(Env::with_parent(env.clone())));
                            loop_env.borrow_mut().define(vars[0].clone(), k);
                            if vars.len() >= 2 {
                                loop_env.borrow_mut().define(vars[1].clone(), v);
                            }
                            match self.eval_block(body, &loop_env)? {
                                Flow::Break => break,
                                Flow::Continue => continue,
                                Flow::Return(v) => return Ok(Flow::Return(v)),
                                Flow::None => {}
                            }
                        }
                    }
                    Value::List(l) => {
                        let items = l.borrow().clone();
                        for (idx, item) in items.into_iter().enumerate() {
                            let loop_env = Rc::new(RefCell::new(Env::with_parent(env.clone())));
                            if vars.len() == 1 {
                                loop_env.borrow_mut().define(vars[0].clone(), item);
                            } else if vars.len() >= 2 {
                                loop_env.borrow_mut().define(vars[0].clone(), Value::Int(idx as i64));
                                loop_env.borrow_mut().define(vars[1].clone(), item);
                            }
                            match self.eval_block(body, &loop_env)? {
                                Flow::Break => break,
                                Flow::Continue => continue,
                                Flow::Return(v) => return Ok(Flow::Return(v)),
                                Flow::None => {}
                            }
                        }
                    }
                    _ => return Err(format!("line {}: 'for' loop over non-iterable value", stmt.line)),
                }
                Ok(Flow::None)
            }
            // `if` and `match` used as statements pass `return`, `break` and `continue` on to the
            // enclosing function or loop (evaluated as values they would swallow them)
            StmtKind::Expr(e) => match &e.kind {
                ExprKind::If { branches, else_body } => {
                    for (cond, body) in branches {
                        if self.eval_expr(cond, env)?.is_truthy() {
                            let block_env = Rc::new(RefCell::new(Env::with_parent(env.clone())));
                            return self.eval_block(body, &block_env);
                        }
                    }
                    if let Some(b) = else_body {
                        let block_env = Rc::new(RefCell::new(Env::with_parent(env.clone())));
                        return self.eval_block(b, &block_env);
                    }
                    Ok(Flow::None)
                }
                ExprKind::Match { scrutinee, arms } => {
                    let val = self.eval_expr(scrutinee, env)?;
                    for arm in arms {
                        if self.match_pattern(&val, &arm.pattern) {
                            let arm_env = Rc::new(RefCell::new(Env::with_parent(env.clone())));
                            return self.eval_block(&arm.body, &arm_env);
                        }
                    }
                    Ok(Flow::None)
                }
                _ => {
                    self.eval_expr(e, env)?;
                    Ok(Flow::None)
                }
            },
        }
    }

    fn apply_assign_op(&self, op: &str, lhs: Value, rhs: Value) -> InterpResult<Value> {
        match op {
            "=" => Ok(rhs),
            "+=" => self.eval_binary("+", lhs, rhs),
            "-=" => self.eval_binary("-", lhs, rhs),
            "*=" => self.eval_binary("*", lhs, rhs),
            "/=" => self.eval_binary("/", lhs, rhs),
            "%=" => self.eval_binary("%", lhs, rhs),
            _ => Err(format!("unknown assignment operator: {}", op)),
        }
    }

    pub fn eval_expr(&mut self, expr: &Expr, env: &Rc<RefCell<Env>>) -> InterpResult<Value> {
        match &expr.kind {
            ExprKind::Int(n) => Ok(Value::Int(*n as i64)),
            ExprKind::Float(f) => Ok(Value::Float(*f)),
            ExprKind::Char(c) => Ok(Value::Char(*c)),
            ExprKind::Bool(b) => Ok(Value::Bool(*b)),
            ExprKind::Nil => Ok(Value::Nil),
            ExprKind::Text(parts) => {
                let mut buf = String::new();
                for p in parts {
                    match p {
                        TextPart::Lit(l) => buf.push_str(l),
                        TextPart::Expr(e) => {
                            let val = self.eval_expr(e, env)?;
                            buf.push_str(&val.to_string());
                        }
                    }
                }
                Ok(Value::Text(buf))
            }
            ExprKind::Ident(name) => {
                if let Some(v) = env.borrow().get(name) {
                    Ok(v)
                } else if let Some(fd) = self.funcs.get(name) {
                    Ok(Value::Func(fd.clone()))
                } else {
                    Err(format!("line {}: undefined identifier '{}'", expr.line, name))
                }
            }
            ExprKind::List(items) => {
                let mut vals = Vec::new();
                for it in items {
                    vals.push(self.eval_expr(it, env)?);
                }
                Ok(Value::List(Rc::new(RefCell::new(vals))))
            }
            ExprKind::Map(entries) => {
                let mut map = MapVal::default();
                for (k, v) in entries {
                    let k_val = self.eval_expr(k, env)?;
                    let v_val = self.eval_expr(v, env)?;
                    map.set(k_val, v_val).map_err(|e| format!("line {}: {}", expr.line, e))?;
                }
                Ok(Value::Map(Rc::new(RefCell::new(map))))
            }
            ExprKind::Record { path, fields } => {
                let type_name = path.join(".");
                let mut field_map = HashMap::new();
                for (fname, fexpr) in fields {
                    field_map.insert(fname.clone(), self.eval_expr(fexpr, env)?);
                }
                // Fill omitted fields with nil if known in type_decl
                if let Some(td) = self.types.get(&type_name) {
                    if let TypeBody::Record(rec_fields) = &td.body {
                        for rf in rec_fields {
                            if !field_map.contains_key(&rf.name) {
                                field_map.insert(rf.name.clone(), Value::Nil);
                            }
                        }
                    }
                }
                Ok(Value::Record {
                    type_name,
                    fields: Rc::new(RefCell::new(field_map)),
                })
            }
            ExprKind::Field { expr: target, name, safe } => {
                let obj = self.eval_expr(target, env)?;
                if *safe && obj == Value::Nil {
                    return Ok(Value::Nil);
                }
                match obj {
                    Value::Record { fields, .. } => {
                        let val = fields.borrow().get(name).cloned().unwrap_or(Value::Nil);
                        Ok(val)
                    }
                    Value::List(l) if name == "len" => Ok(Value::Int(l.borrow().len() as i64)),
                    Value::Text(t) if name == "len" => Ok(Value::Int(t.len() as i64)),
                    Value::Map(m) if name == "len" => Ok(Value::Int(m.borrow().len() as i64)),
                    _ => Err(format!("line {}: cannot access field '{}' on {}", expr.line, name, obj.type_name())),
                }
            }
            ExprKind::Index { expr: target, index } => {
                let obj = self.eval_expr(target, env)?;
                let idx = self.eval_expr(index, env)?;
                match (obj, idx) {
                    (Value::List(l), Value::Int(i)) => {
                        let list = l.borrow();
                        let idx = i as usize;
                        if idx < list.len() {
                            Ok(list[idx].clone())
                        } else {
                            Err(format!("line {}: index out of bounds: {} (len {})", expr.line, i, list.len()))
                        }
                    }
                    (Value::Map(m), key) => {
                        let found = m.borrow().get(&key).map_err(|e| format!("line {}: {}", expr.line, e))?;
                        found.ok_or_else(|| format!("line {}: map key not found", expr.line))
                    }
                    (Value::Text(s), Value::Int(i)) => {
                        let chars: Vec<char> = s.chars().collect();
                        let idx = i as usize;
                        if idx < chars.len() {
                            Ok(Value::Char(chars[idx]))
                        } else {
                            Err(format!("line {}: string index out of bounds", expr.line))
                        }
                    }
                    (obj, _) => Err(format!("line {}: cannot index {}", expr.line, obj.type_name())),
                }
            }
            ExprKind::Unary { op, expr: inner } => {
                let val = self.eval_expr(inner, env)?;
                match *op {
                    "not" => Ok(Value::Bool(!val.is_truthy())),
                    "-" => match val {
                        Value::Int(n) => Ok(Value::Int(-n)),
                        Value::Float(f) => Ok(Value::Float(-f)),
                        _ => Err(format!("line {}: unary '-' expects number", expr.line)),
                    },
                    "bitnot" => match val {
                        Value::Int(n) => Ok(Value::Int(!n)),
                        _ => Err(format!("line {}: 'bitnot' expects integer", expr.line)),
                    },
                    "&" | "*" => Ok(val),
                    _ => Err(format!("line {}: unknown unary op: {}", expr.line, op)),
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                if *op == "and" {
                    let left = self.eval_expr(lhs, env)?;
                    if !left.is_truthy() {
                        return Ok(left);
                    }
                    return self.eval_expr(rhs, env);
                }
                if *op == "or" {
                    let left = self.eval_expr(lhs, env)?;
                    if left.is_truthy() {
                        return Ok(left);
                    }
                    return self.eval_expr(rhs, env);
                }
                if *op == "??" {
                    let left = self.eval_expr(lhs, env)?;
                    if left != Value::Nil {
                        return Ok(left);
                    }
                    return self.eval_expr(rhs, env);
                }

                let left = self.eval_expr(lhs, env)?;
                let right = self.eval_expr(rhs, env)?;
                self.eval_binary(op, left, right)
            }
            ExprKind::Call { callee, args } => {
                // Check if it's a method call: target.method(args)
                if let ExprKind::Field { expr: target_expr, name: method_name, .. } = &callee.kind {
                    let target_val = self.eval_expr(target_expr, env)?;
                    let mut eval_args = Vec::new();
                    for a in args {
                        eval_args.push(self.eval_expr(&a.value, env)?);
                    }
                    return self.call_method(target_val, method_name, eval_args, expr.line);
                }

                // Regular function call
                let func_val = self.eval_expr(callee, env)?;
                let mut eval_args = Vec::new();
                for a in args {
                    eval_args.push(self.eval_expr(&a.value, env)?);
                }

                match func_val {
                    Value::Builtin(name) => self.call_builtin(&name, eval_args, expr.line),
                    Value::Func(fd) => self.call_func(&fd, eval_args),
                    _ => Err(format!("line {}: calling non-function value: {}", expr.line, func_val)),
                }
            }
            ExprKind::If { branches, else_body } => {
                for (cond, body) in branches {
                    if self.eval_expr(cond, env)?.is_truthy() {
                        let block_env = Rc::new(RefCell::new(Env::with_parent(env.clone())));
                        match self.eval_block(body, &block_env)? {
                            Flow::Return(v) => return Ok(v),
                            _ => return Ok(Value::Nil),
                        }
                    }
                }
                if let Some(else_b) = else_body {
                    let block_env = Rc::new(RefCell::new(Env::with_parent(env.clone())));
                    match self.eval_block(else_b, &block_env)? {
                        Flow::Return(v) => return Ok(v),
                        _ => return Ok(Value::Nil),
                    }
                }
                Ok(Value::Nil)
            }
            ExprKind::Await(_) => Err(format!("line {}: 'await' needs the native compiler (use -o)", expr.line)),
            ExprKind::Propagate(inner) => {
                let v = self.eval_expr(inner, env)?;
                Ok(v)
            }
            ExprKind::Deref(inner) => self.eval_expr(inner, env),
            ExprKind::New(_) => Ok(Value::Nil),
            ExprKind::Lambda { params, body } => {
                let fd = FuncDecl {
                    receiver: None,
                    name: "<lambda>".to_string(),
                    generics: Vec::new(),
                    params: params.clone(),
                    ret: None,
                    body: body.clone(),
                    is_async: false,
                };
                Ok(Value::Func(fd))
            }
            ExprKind::Match { scrutinee, arms } => {
                let val = self.eval_expr(scrutinee, env)?;
                for arm in arms {
                    if self.match_pattern(&val, &arm.pattern) {
                        let arm_env = Rc::new(RefCell::new(Env::with_parent(env.clone())));
                        match self.eval_block(&arm.body, &arm_env)? {
                            Flow::Return(v) => return Ok(v),
                            _ => return Ok(Value::Nil),
                        }
                    }
                }
                Ok(Value::Nil)
            }
        }
    }

    fn match_pattern(&self, val: &Value, patterns: &[PatAtom]) -> bool {
        for pat in patterns {
            match pat {
                PatAtom::Wildcard => return true,
                PatAtom::Literal(lit_expr) => {
                    match (&lit_expr.kind, val) {
                        (ExprKind::Int(n), Value::Int(i)) if (*n as i64) == *i => return true,
                        (ExprKind::Bool(b1), Value::Bool(b2)) if b1 == b2 => return true,
                        _ => {}
                    }
                }
                PatAtom::Name { name, .. } => {
                    if let Value::Record { type_name, .. } = val {
                        if type_name == name {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    fn eval_binary(&self, op: &str, left: Value, right: Value) -> InterpResult<Value> {
        match op {
            ".." => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Range(a, b)),
                _ => Err("range '..' expects integers".to_string()),
            },
            "+" => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
                (Value::Text(a), b) => Ok(Value::Text(format!("{}{}", a, b))),
                (a, Value::Text(b)) => Ok(Value::Text(format!("{}{}", a, b))),
                _ => Err("invalid operands for '+'".to_string()),
            },
            "-" => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a - b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
                _ => Err("invalid operands for '-'".to_string()),
            },
            "*" => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a * b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
                _ => Err("invalid operands for '*'".to_string()),
            },
            "/" => match (left, right) {
                (Value::Int(a), Value::Int(b)) => {
                    if b == 0 {
                        Err("division by zero".to_string())
                    } else {
                        Ok(Value::Int(a / b))
                    }
                }
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a / b)),
                _ => Err("invalid operands for '/'".to_string()),
            },
            "%" => match (left, right) {
                (Value::Int(a), Value::Int(b)) => {
                    if b == 0 {
                        Err("modulo by zero".to_string())
                    } else {
                        Ok(Value::Int(a % b))
                    }
                }
                _ => Err("modulo '%' expects integers".to_string()),
            },
            "bitand" => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a & b)),
                _ => Err("'bitand' expects integers".to_string()),
            },
            "bitor" => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a | b)),
                _ => Err("'bitor' expects integers".to_string()),
            },
            "bitxor" => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a ^ b)),
                _ => Err("'bitxor' expects integers".to_string()),
            },
            "<<" => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a << b)),
                _ => Err("'<<' expects integers".to_string()),
            },
            ">>" => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a >> b)),
                _ => Err("'>>' expects integers".to_string()),
            },
            "==" => Ok(Value::Bool(left == right)),
            "!=" => Ok(Value::Bool(left != right)),
            "<" => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a < b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a < b)),
                _ => Err("invalid operands for '<'".to_string()),
            },
            "<=" => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a <= b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a <= b)),
                _ => Err("invalid operands for '<='".to_string()),
            },
            ">" => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a > b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a > b)),
                _ => Err("invalid operands for '>'".to_string()),
            },
            ">=" => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a >= b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a >= b)),
                _ => Err("invalid operands for '>='".to_string()),
            },
            _ => Err(format!("unsupported binary operator '{}'", op)),
        }
    }

    fn call_builtin(&self, name: &str, args: Vec<Value>, _line: u32) -> InterpResult<Value> {
        match name {
            "print" | "println" => {
                let out: Vec<String> = args.iter().map(|a| a.to_string()).collect();
                println!("{}", out.join(" "));
                Ok(Value::Nil)
            }
            "len" => {
                if let Some(first) = args.first() {
                    match first {
                        Value::List(l) => Ok(Value::Int(l.borrow().len() as i64)),
                        Value::Text(t) => Ok(Value::Int(t.len() as i64)),
                        Value::Map(m) => Ok(Value::Int(m.borrow().len() as i64)),
                        _ => Err("len() expects list, map or text".to_string()),
                    }
                } else {
                    Err("len() requires 1 argument".to_string())
                }
            }
            "float" | "f64" | "f32" | "int" | "i64" => {
                let [a] = args.as_slice() else { return Err(format!("{}() takes one argument", name)) };
                match (name, a) {
                    ("float" | "f64", Value::Int(n)) => Ok(Value::Float(*n as f64)),
                    ("f32", Value::Int(n)) => Ok(Value::Float(*n as f32 as f64)),
                    ("float" | "f64", Value::Float(f)) => Ok(Value::Float(*f)),
                    ("f32", Value::Float(f)) => Ok(Value::Float(*f as f32 as f64)),
                    ("int" | "i64", Value::Int(n)) => Ok(Value::Int(*n)),
                    ("int" | "i64", Value::Float(f)) => {
                        if f.is_nan() || *f >= 9223372036854775808.0 || *f < -9223372036854775808.0 {
                            Err("float out of range".to_string())
                        } else {
                            Ok(Value::Int(f.trunc() as i64))
                        }
                    }
                    _ => Err(format!("cannot convert {} to {}", a.type_name(), name)),
                }
            }
            _ => Err(format!("unknown builtin function: {}", name)),
        }
    }

    fn call_func(&mut self, fd: &FuncDecl, args: Vec<Value>) -> InterpResult<Value> {
        let env = Rc::new(RefCell::new(Env::with_parent(self.globals.clone())));
        for (i, param) in fd.params.iter().enumerate() {
            let val = args.get(i).cloned().unwrap_or(Value::Nil);
            env.borrow_mut().define(param.name.clone(), val);
        }
        match self.run_body(&fd.body, &env)? {
            Flow::Return(v) => Ok(v),
            _ => Ok(Value::Nil),
        }
    }

    fn call_method(
        &mut self,
        target: Value,
        method: &str,
        args: Vec<Value>,
        line: u32,
    ) -> InterpResult<Value> {
        if let Value::List(l) = &target {
            if method == "add" || method == "push" {
                if let Some(item) = args.first() {
                    l.borrow_mut().push(item.clone());
                    return Ok(Value::Nil);
                }
            }
        }

        if let Value::Map(m) = &target {
            let want = match method {
                "clear" => 0,
                "get" => 2,
                "has" | "remove" => 1,
                _ => return Err(format!("line {}: no method '{}' found on map", line, method)),
            };
            if args.len() != want {
                return Err(format!("line {}: '{}' takes {} argument(s)", line, method, want));
            }
            let key_err = |e: String| format!("line {}: {}", line, e);
            return match method {
                "has" => Ok(Value::Bool(m.borrow().get(&args[0]).map_err(key_err)?.is_some())),
                "get" => Ok(m.borrow().get(&args[0]).map_err(key_err)?.unwrap_or_else(|| args[1].clone())),
                "remove" => m.borrow_mut().remove(&args[0]).map(|_| Value::Nil).map_err(key_err),
                _ => {
                    m.borrow_mut().clear();
                    Ok(Value::Nil)
                }
            };
        }

        if let Value::Record { type_name, .. } = &target {
            if let Some(fd) = self.methods.get(&(type_name.clone(), method.to_string())).cloned() {
                let env = Rc::new(RefCell::new(Env::with_parent(self.globals.clone())));
                if let Some(rec) = &fd.receiver {
                    env.borrow_mut().define(rec.name.clone(), target);
                }
                for (i, param) in fd.params.iter().enumerate() {
                    let val = args.get(i).cloned().unwrap_or(Value::Nil);
                    env.borrow_mut().define(param.name.clone(), val);
                }
                match self.run_body(&fd.body, &env)? {
                    Flow::Return(v) => return Ok(v),
                    _ => return Ok(Value::Nil),
                }
            }
        }

        Err(format!("line {}: no method '{}' found on {}", line, method, target.type_name()))
    }
}

/// Digits and layout of a float as the native `float.to_text` prints it: the shortest text that
/// reads back as the same number, plain between 1e-4 and 1e16, exponent form outside.
pub fn float_text(f: f64) -> String {
    layout_float(f.is_nan(), f.is_sign_negative(), f.is_infinite(), f == 0.0, || format!("{:e}", f.abs()))
}

/// The same for an `f32` value.
pub fn float_text_f32(f: f32) -> String {
    layout_float(f.is_nan(), f.is_sign_negative(), f.is_infinite(), f == 0.0, || format!("{:e}", f.abs()))
}

fn layout_float(nan: bool, neg: bool, inf: bool, zero: bool, sci: impl Fn() -> String) -> String {
    if nan {
        return "NaN".into();
    }
    let sign = if neg { "-" } else { "" };
    if inf {
        return format!("{}inf", sign);
    }
    if zero {
        return format!("{}0.0", sign);
    }
    let sci = sci();
    let (mant, exp) = sci.split_once('e').unwrap();
    let x: i32 = exp.parse().unwrap();
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let n = digits.len() as i32;
    if x >= 16 || x < -4 {
        let rest = &digits[1..];
        let m = if rest.is_empty() { digits[..1].to_string() } else { format!("{}.{}", &digits[..1], rest) };
        return format!("{}{}e{}", sign, m, x);
    }
    if x < 0 {
        return format!("{}0.{}{}", sign, "0".repeat((-x - 1) as usize), digits);
    }
    if n <= x + 1 {
        return format!("{}{}{}.0", sign, digits, "0".repeat((x + 1 - n) as usize));
    }
    format!("{}{}.{}", sign, &digits[..(x + 1) as usize], &digits[(x + 1) as usize..])
}
