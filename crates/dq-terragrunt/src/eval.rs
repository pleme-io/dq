//! Terragrunt expression evaluation context.
//!
//! Wraps `hcl::eval::Context` and registers Terragrunt built-in functions
//! (`find_in_parent_folders`, `path_relative_to_include`, `get_terragrunt_dir`,
//! `get_parent_terragrunt_dir`) so that HCL expressions containing these
//! function calls can be evaluated without the Terragrunt Go runtime.

use dq_core::Value;
use dq_formats::hcl::{expression_func_name, is_hcl_expression};
use hcl::eval::{Context, FuncArgs, FuncDef, ParamType};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Evaluation context for a single Terragrunt module.
///
/// Holds the module directory and optional include directory so the built-in
/// Terragrunt functions can resolve paths relative to the module tree.
pub struct TerragruntEvalContext<'a> {
    /// The `hcl::eval::Context` with registered functions and variables.
    pub ctx: Context<'a>,
    /// Absolute path to the module directory (where the leaf `terragrunt.hcl` lives).
    pub module_dir: PathBuf,
    /// Absolute path to the include (parent) directory, if any.
    pub include_dir: Option<PathBuf>,
}

// We pass directory paths into the stateless hcl FuncDef callbacks via
// thread-local storage because `Func = fn(FuncArgs) -> Result<Value, String>`
// cannot capture state.
thread_local! {
    static TG_MODULE_DIR: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
    static TG_INCLUDE_DIR: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

fn set_thread_dirs(module_dir: &Path, include_dir: Option<&Path>) {
    TG_MODULE_DIR.with(|cell| *cell.borrow_mut() = Some(module_dir.to_path_buf()));
    TG_INCLUDE_DIR.with(|cell| *cell.borrow_mut() = include_dir.map(|p| p.to_path_buf()));
}

fn get_module_dir() -> Result<PathBuf, String> {
    TG_MODULE_DIR.with(|cell| {
        cell.borrow()
            .clone()
            .ok_or_else(|| "get_terragrunt_dir: module_dir not set".to_string())
    })
}

fn get_include_dir() -> Result<PathBuf, String> {
    TG_INCLUDE_DIR.with(|cell| {
        cell.borrow()
            .clone()
            .ok_or_else(|| "get_parent_terragrunt_dir: include_dir not set".to_string())
    })
}

/// `find_in_parent_folders(filename?)` — walks up from `module_dir` looking
/// for a file. Default filename is `"terragrunt.hcl"`.  Returns the absolute
/// path to the found file, or an error if it reaches the filesystem root.
fn find_in_parent_folders(args: FuncArgs) -> Result<hcl::Value, String> {
    let filename = if args.is_empty() {
        "terragrunt.hcl".to_string()
    } else {
        args[0]
            .as_str()
            .ok_or("find_in_parent_folders: expected string argument")?
            .to_string()
    };

    let module_dir = get_module_dir()?;
    let mut dir = module_dir.clone();

    loop {
        if !dir.pop() {
            return Err(format!(
                "find_in_parent_folders: {filename} not found above {}",
                module_dir.display()
            ));
        }
        let candidate = dir.join(&filename);
        if candidate.exists() {
            return Ok(hcl::Value::from(
                candidate.to_string_lossy().into_owned(),
            ));
        }
    }
}

/// `path_relative_to_include()` — computes the relative path from the include
/// directory to the module directory.
fn path_relative_to_include(_args: FuncArgs) -> Result<hcl::Value, String> {
    let module_dir = get_module_dir()?;
    let include_dir = get_include_dir()?;
    let rel = pathdiff(&module_dir, &include_dir);
    Ok(hcl::Value::from(rel))
}

/// `get_terragrunt_dir()` — returns the absolute path to the module directory.
fn get_terragrunt_dir_fn(_args: FuncArgs) -> Result<hcl::Value, String> {
    let dir = get_module_dir()?;
    Ok(hcl::Value::from(dir.to_string_lossy().into_owned()))
}

/// `get_parent_terragrunt_dir()` — returns the absolute path to the include
/// (parent) module directory.
fn get_parent_terragrunt_dir_fn(_args: FuncArgs) -> Result<hcl::Value, String> {
    let dir = get_include_dir()?;
    Ok(hcl::Value::from(dir.to_string_lossy().into_owned()))
}

/// Simple relative-path computation (avoid pulling in the `pathdiff` crate).
/// Returns the relative path from `base` to `target`.
fn pathdiff(target: &Path, base: &Path) -> String {
    // Normalize both to absolute (they should already be absolute).
    let target_components: Vec<_> = target.components().collect();
    let base_components: Vec<_> = base.components().collect();

    let common_len = target_components
        .iter()
        .zip(base_components.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let ups = base_components.len() - common_len;
    let mut parts: Vec<String> = (0..ups).map(|_| "..".to_string()).collect();
    for component in &target_components[common_len..] {
        parts.push(component.as_os_str().to_string_lossy().into_owned());
    }

    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

impl<'a> TerragruntEvalContext<'a> {
    /// Create a new evaluation context for the given module directory.
    ///
    /// If the module has an include, pass its resolved directory as
    /// `include_dir` so that `path_relative_to_include` and
    /// `get_parent_terragrunt_dir` work correctly.
    pub fn new(module_dir: PathBuf, include_dir: Option<PathBuf>) -> Self {
        let mut ctx = Context::new();

        // find_in_parent_folders(filename?) — 0 required params, 1 variadic string
        let fipf = FuncDef::builder()
            .variadic_param(ParamType::String)
            .build(find_in_parent_folders);
        ctx.declare_func("find_in_parent_folders", fipf);

        // path_relative_to_include() — 0 params
        let prti = FuncDef::builder().build(path_relative_to_include);
        ctx.declare_func("path_relative_to_include", prti);

        // get_terragrunt_dir() — 0 params
        let gtd = FuncDef::builder().build(get_terragrunt_dir_fn);
        ctx.declare_func("get_terragrunt_dir", gtd);

        // get_parent_terragrunt_dir() — 0 params
        let gptd = FuncDef::builder().build(get_parent_terragrunt_dir_fn);
        ctx.declare_func("get_parent_terragrunt_dir", gptd);

        TerragruntEvalContext {
            ctx,
            module_dir,
            include_dir,
        }
    }

    /// Install thread-local state before evaluating expressions.
    /// Must be called before any call to `evaluate_value`.
    fn install_thread_state(&self) {
        set_thread_dirs(&self.module_dir, self.include_dir.as_deref());
    }
}

/// Recursively walk a `dq_core::Value` tree, evaluating any nodes that
/// represent unevaluated HCL expressions (identified by the `__expr` key
/// injected by `dq-formats`).
///
/// Nodes that cannot be evaluated (e.g. unknown function, missing variable)
/// are left unchanged.
pub fn evaluate_value(value: &Value, eval_ctx: &TerragruntEvalContext) -> Value {
    eval_ctx.install_thread_state();
    evaluate_inner(value, eval_ctx)
}

fn evaluate_inner(value: &Value, eval_ctx: &TerragruntEvalContext) -> Value {
    match value {
        Value::Map(_) if is_hcl_expression(value) => try_evaluate_expr(value, eval_ctx),
        Value::Map(m) => {
            let mut new_map = indexmap::IndexMap::new();
            for (k, v) in m.iter() {
                new_map.insert(k.clone(), evaluate_inner(v, eval_ctx));
            }
            Value::map(new_map)
        }
        Value::Array(a) => {
            Value::array(a.iter().map(|v| evaluate_inner(v, eval_ctx)).collect::<Vec<_>>())
        }
        Value::Block(b) => {
            let mut new_body = indexmap::IndexMap::new();
            for (k, v) in b.body.iter() {
                new_body.insert(k.clone(), evaluate_inner(v, eval_ctx));
            }
            Value::Block(dq_core::value::Block {
                block_type: b.block_type.clone(),
                labels: b.labels.clone(),
                body: Arc::new(new_body),
            })
        }
        other => other.clone(),
    }
}

/// Attempt to evaluate a single expression node.  Falls back to the original
/// value if evaluation fails.
fn try_evaluate_expr(expr_value: &Value, eval_ctx: &TerragruntEvalContext) -> Value {
    let expr_type = match expr_value.get("__expr").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return expr_value.clone(),
    };

    match expr_type {
        "func_call" => try_evaluate_func_call(expr_value, eval_ctx),
        "template" => try_evaluate_template(expr_value, eval_ctx),
        "variable" => try_evaluate_variable(expr_value, eval_ctx),
        "traversal" => try_evaluate_traversal(expr_value, eval_ctx),
        _ => expr_value.clone(),
    }
}

/// Parse an HCL assignment, evaluate it through the context, and extract
/// the resulting attribute value.  Returns `None` if any step fails.
///
/// All `try_evaluate_*` helpers delegate here to avoid duplicating the
/// parse -> evaluate -> extract pipeline.
fn eval_hcl_assignment(hcl_text: &str, eval_ctx: &TerragruntEvalContext) -> Option<Value> {
    use hcl::eval::Evaluate;
    let body = hcl::from_str::<hcl::Body>(hcl_text).ok()?;
    let evaluated_body = body.evaluate(&eval_ctx.ctx).ok()?;
    for structure in evaluated_body.iter() {
        if let hcl::Structure::Attribute(attr) = structure {
            return Some(dq_formats::hcl::from_hcl_value(
                &hcl::Value::try_from(attr.expr.clone()).unwrap_or(hcl::Value::Null),
            ));
        }
    }
    None
}

/// Evaluate `func_call` expression nodes.
fn try_evaluate_func_call(expr_value: &Value, eval_ctx: &TerragruntEvalContext) -> Value {
    let func_name = match expression_func_name(expr_value) {
        Some(n) => n,
        None => return expr_value.clone(),
    };

    let args = match expr_value.get("args").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return expr_value.clone(),
    };

    // Evaluate arguments first (they may themselves be expressions)
    let evaluated_args: Vec<Value> = args
        .iter()
        .map(|a| evaluate_inner(a, eval_ctx))
        .collect();

    // Build a mini HCL expression string and evaluate through the context
    let arg_strs: Vec<String> = evaluated_args
        .iter()
        .map(|a| match a {
            Value::String(s) => format!("\"{}\"", s),
            Value::Int(n) => format!("{n}"),
            Value::Float(f) => format!("{f}"),
            Value::Bool(b) => format!("{b}"),
            Value::Null => "null".to_string(),
            _ => "null".to_string(),
        })
        .collect();

    let hcl_expr_str = format!("{}({})", func_name, arg_strs.join(", "));
    let hcl_text = format!("_v = {hcl_expr_str}");

    eval_hcl_assignment(&hcl_text, eval_ctx).unwrap_or_else(|| expr_value.clone())
}

/// Evaluate template expression nodes (`"${func()}-suffix"` etc.)
fn try_evaluate_template(expr_value: &Value, eval_ctx: &TerragruntEvalContext) -> Value {
    let template_str = match expr_value.get("template").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return expr_value.clone(),
    };

    let hcl_text = format!("_v = \"{}\"", template_str);
    eval_hcl_assignment(&hcl_text, eval_ctx).unwrap_or_else(|| expr_value.clone())
}

/// Evaluate a bare variable reference.
fn try_evaluate_variable(expr_value: &Value, eval_ctx: &TerragruntEvalContext) -> Value {
    let var_name = match expr_value.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return expr_value.clone(),
    };

    let hcl_text = format!("_v = {var_name}");
    eval_hcl_assignment(&hcl_text, eval_ctx).unwrap_or_else(|| expr_value.clone())
}

/// Evaluate a traversal expression (e.g. `local.foo`).
fn try_evaluate_traversal(expr_value: &Value, eval_ctx: &TerragruntEvalContext) -> Value {
    // Reconstruct the traversal as HCL text
    let root = match expr_value.get("root") {
        Some(r) => r,
        None => return expr_value.clone(),
    };
    let ops = match expr_value.get("operators").and_then(|v| v.as_array()) {
        Some(o) => o,
        None => return expr_value.clone(),
    };

    let root_name = match root.get("name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => return expr_value.clone(),
    };

    let mut traversal_str = root_name;
    for op in ops {
        let op_type = match op.get("type").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => return expr_value.clone(),
        };
        match op_type {
            "get_attr" => {
                let name = match op.get("name").and_then(|v| v.as_str()) {
                    Some(n) => n,
                    None => return expr_value.clone(),
                };
                traversal_str = format!("{traversal_str}.{name}");
            }
            _ => return expr_value.clone(),
        }
    }

    let hcl_text = format!("_v = {traversal_str}");
    eval_hcl_assignment(&hcl_text, eval_ctx).unwrap_or_else(|| expr_value.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eval_get_terragrunt_dir() {
        let module_dir = PathBuf::from("/tmp/infra/modules/vpc");
        let eval_ctx = TerragruntEvalContext::new(module_dir.clone(), None);
        eval_ctx.install_thread_state();

        // Simulate a func_call expression node
        let expr = Value::from_pairs([
            ("__expr", Value::string("func_call")),
            ("name", Value::string("get_terragrunt_dir")),
            ("args", Value::array(vec![])),
        ]);

        let result = evaluate_value(&expr, &eval_ctx);
        assert_eq!(result.as_str(), Some("/tmp/infra/modules/vpc"));
    }

    #[test]
    fn eval_get_parent_terragrunt_dir() {
        let module_dir = PathBuf::from("/tmp/infra/modules/vpc");
        let include_dir = PathBuf::from("/tmp/infra");
        let eval_ctx =
            TerragruntEvalContext::new(module_dir.clone(), Some(include_dir.clone()));
        eval_ctx.install_thread_state();

        let expr = Value::from_pairs([
            ("__expr", Value::string("func_call")),
            ("name", Value::string("get_parent_terragrunt_dir")),
            ("args", Value::array(vec![])),
        ]);

        let result = evaluate_value(&expr, &eval_ctx);
        assert_eq!(result.as_str(), Some("/tmp/infra"));
    }

    #[test]
    fn eval_path_relative_to_include() {
        let module_dir = PathBuf::from("/tmp/infra/envs/prod/vpc");
        let include_dir = PathBuf::from("/tmp/infra");
        let eval_ctx =
            TerragruntEvalContext::new(module_dir.clone(), Some(include_dir.clone()));
        eval_ctx.install_thread_state();

        let expr = Value::from_pairs([
            ("__expr", Value::string("func_call")),
            ("name", Value::string("path_relative_to_include")),
            ("args", Value::array(vec![])),
        ]);

        let result = evaluate_value(&expr, &eval_ctx);
        assert_eq!(result.as_str(), Some("envs/prod/vpc"));
    }

    #[test]
    fn eval_find_in_parent_folders() {
        // Create a temp directory structure
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("infra");
        let child = root.join("envs").join("prod");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(root.join("terragrunt.hcl"), "# root config\n").unwrap();

        let eval_ctx = TerragruntEvalContext::new(child.clone(), None);
        eval_ctx.install_thread_state();

        let expr = Value::from_pairs([
            ("__expr", Value::string("func_call")),
            ("name", Value::string("find_in_parent_folders")),
            ("args", Value::array(vec![])),
        ]);

        let result = evaluate_value(&expr, &eval_ctx);
        let result_str = result.as_str().unwrap();
        assert!(result_str.contains("infra"));
        assert!(result_str.ends_with("terragrunt.hcl"));
    }

    #[test]
    fn eval_find_in_parent_folders_custom_filename() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("infra");
        let child = root.join("envs").join("prod");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(root.join("common.hcl"), "# common\n").unwrap();

        let eval_ctx = TerragruntEvalContext::new(child.clone(), None);
        eval_ctx.install_thread_state();

        let expr = Value::from_pairs([
            ("__expr", Value::string("func_call")),
            ("name", Value::string("find_in_parent_folders")),
            ("args", Value::array(vec![Value::string("common.hcl")])),
        ]);

        let result = evaluate_value(&expr, &eval_ctx);
        let result_str = result.as_str().unwrap();
        assert!(result_str.ends_with("common.hcl"));
    }

    #[test]
    fn eval_non_expression_passthrough() {
        let eval_ctx = TerragruntEvalContext::new(PathBuf::from("/tmp"), None);

        let plain = Value::string("hello");
        assert_eq!(evaluate_value(&plain, &eval_ctx), Value::string("hello"));

        let map = Value::from_pairs([
            ("key", Value::string("value")),
            ("num", Value::int(42)),
        ]);
        assert_eq!(evaluate_value(&map, &eval_ctx), map);
    }

    #[test]
    fn eval_pathdiff_same_dir() {
        assert_eq!(pathdiff(Path::new("/a/b/c"), Path::new("/a/b/c")), ".");
    }

    #[test]
    fn eval_pathdiff_relative() {
        assert_eq!(
            pathdiff(Path::new("/a/b/c/d"), Path::new("/a/b")),
            "c/d"
        );
        assert_eq!(
            pathdiff(Path::new("/a/b"), Path::new("/a/b/c/d")),
            "../.."
        );
    }
}
