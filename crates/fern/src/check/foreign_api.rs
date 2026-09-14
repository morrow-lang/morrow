//! Checked conversions preserve narrow scalar ranges and retain borrowed pointer ownership.
use super::*;
use ir::{Expr as E, ExprKind as K};
fn expression(kind: K, ty: Type, span: Span) -> E {
    E { kind, ty, span }
}
fn result(ty: Type) -> Type {
    Type::Result(Box::new(ty), Box::new(Type::String))
}
fn pointer(item: Type) -> Type {
    Type::Named("Ptr".into(), vec![item])
}
fn byte_pointer() -> Type {
    pointer(Type::Named("CUInt8".into(), vec![]))
}
fn field(value: E, index: usize, ty: Type) -> E {
    let span = value.span;
    expression(
        K::Field {
            value: Box::new(value),
            index,
        },
        ty,
        span,
    )
}
impl Checker<'_> {
    fn foreign_api_signature(&mut self, name: &str) -> (Vec<Type>, Type) {
        let pointer = pointer(self.inference.fresh());
        match name {
            "Ptr.null" => (vec![], pointer),
            "Ptr.is_null" => (vec![pointer], Type::Bool),
            "Ptr.equal" => (vec![pointer.clone(), pointer], Type::Bool),
            "Ptr.to_string" => (vec![byte_pointer(), Type::Int], result(Type::String)),
            "String.as_ptr" => (vec![Type::String], byte_pointer()),
            _ => {
                let (owner, method) = name.split_once('.').expect("validated foreign API");
                let nominal = Type::Named(owner.into(), vec![]);
                let primitive = if owner == "CFloat32" {
                    Type::Float
                } else {
                    Type::Int
                };
                if method.starts_with("from_") {
                    (vec![primitive], result(nominal))
                } else {
                    (
                        vec![nominal],
                        if owner == "CUInt64" {
                            result(primitive)
                        } else {
                            primitive
                        },
                    )
                }
            }
        }
    }
    pub(super) fn foreign_api(
        &mut self,
        name: &str,
        args: &[ast::Argument],
        expected: Option<&Type>,
        span: Span,
        depth: usize,
    ) -> Checked<TypedKind> {
        labels::positional(args)?;
        let (params, result) = self.foreign_api_signature(name);
        self.constrain_result(&result, expected, span)?;
        let args = self.call_arguments(args, &params, span, depth)?;
        let value = self.foreign_api_body(name, args, result, span)?;
        Ok((value.kind, value.ty))
    }
    pub(super) fn foreign_api_value(&mut self, name: &str, span: Span) -> Checked<TypedKind> {
        let (types, result) = self.foreign_api_signature(name);
        let params: Vec<_> = types
            .iter()
            .map(|ty| {
                let id = ir::LocalId(self.local_count);
                self.local_count += 1;
                ir::Param { id, ty: ty.clone() }
            })
            .collect();
        let args = params
            .iter()
            .map(|p| expression(K::Local(p.id), p.ty.clone(), span))
            .collect();
        let body = self.foreign_api_body(name, args, result.clone(), span)?;
        Ok((
            K::Lambda {
                params,
                captures: vec![],
                body: Box::new(body),
                local_count: self.local_count,
            },
            Type::Function(types, Box::new(result)),
        ))
    }
    pub(super) fn foreign_string_borrow(
        &mut self,
        value: E,
        args: &[ast::Argument],
        span: Span,
    ) -> Checked<TypedKind> {
        if !args.is_empty() {
            return Err(Diagnostic::new(
                span,
                "String.as_ptr method takes no arguments",
            ));
        }
        self.inference
            .unify(&value.ty, &Type::String, span, "String.as_ptr receiver")?;
        let value = self.foreign_api_body("String.as_ptr", vec![value], byte_pointer(), span)?;
        Ok((value.kind, value.ty))
    }
    fn foreign_api_body(
        &mut self,
        name: &str,
        args: Vec<E>,
        result: Type,
        span: Span,
    ) -> Checked<E> {
        if let Some(index) = args.iter().position(|a| a.ty == Type::Never) {
            return Ok(expression(
                K::Block(
                    args.into_iter()
                        .take(index + 1)
                        .map(ir::Stmt::Expr)
                        .collect(),
                ),
                Type::Never,
                span,
            ));
        }
        let mut statements = Vec::new();
        let args: Vec<_> = args
            .into_iter()
            .map(|value| {
                let id = ir::LocalId(self.local_count);
                self.local_count += 1;
                let local = expression(K::Local(id), value.ty.clone(), span);
                statements.push(ir::Stmt::Let { id, value });
                local
            })
            .collect();
        let value = match name {
            "Ptr.null" => expression(
                K::CustomConstruct {
                    tag: 0,
                    fields: vec![
                        expression(K::Int(0), Type::Int, span),
                        expression(K::List(vec![]), Type::List(Box::new(Type::String)), span),
                    ],
                },
                result,
                span,
            ),
            "Ptr.is_null" => compare(
                ast::BinaryOp::Eq,
                field(args[0].clone(), 0, Type::Int),
                expression(K::Int(0), Type::Int, span),
                span,
            ),
            "Ptr.equal" => compare(
                ast::BinaryOp::Eq,
                field(args[0].clone(), 0, Type::Int),
                field(args[1].clone(), 0, Type::Int),
                span,
            ),
            "String.as_ptr" => internal("$ffi.borrow_string", args, result, span)?,
            "Ptr.to_string" => internal(
                "$ffi.read_string",
                vec![field(args[0].clone(), 0, Type::Int), args[1].clone()],
                result,
                span,
            )?,
            _ => scalar_conversion(name, args[0].clone(), result, span)?,
        };
        let ty = value.ty.clone();
        statements.push(ir::Stmt::Expr(value));
        Ok(expression(K::Block(statements), ty, span))
    }
}
fn internal(name: &str, args: Vec<E>, ty: Type, span: Span) -> Checked<E> {
    let id = runtime::resolve(name)
        .ok_or_else(|| Diagnostic::new(span, "missing internal foreign runtime boundary"))?;
    Ok(expression(
        K::Call {
            target: ir::CallTarget::Runtime(id),
            args,
        },
        ty,
        span,
    ))
}
fn compare(op: ast::BinaryOp, left: E, right: E, span: Span) -> E {
    expression(
        K::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        },
        Type::Bool,
        span,
    )
}
fn checked_value(condition: E, value: E, result: Type, span: Span) -> E {
    let ok = expression(
        K::Construct {
            constructor: Constructor::Ok,
            value: Some(Box::new(value)),
        },
        result.clone(),
        span,
    );
    let error = expression(
        K::String("foreign scalar conversion is outside its representable range".into()),
        Type::String,
        span,
    );
    let error = expression(
        K::Construct {
            constructor: Constructor::Err,
            value: Some(Box::new(error)),
        },
        result.clone(),
        span,
    );
    expression(
        K::If {
            condition: Box::new(condition),
            then_branch: Box::new(ok),
            else_branch: Some(Box::new(error)),
        },
        result,
        span,
    )
}
fn scalar_conversion(name: &str, value: E, result: Type, span: Span) -> Checked<E> {
    let (owner, method) = name.split_once('.').expect("validated scalar API");
    if method.starts_with("to_") {
        let inner = if owner == "CFloat32" {
            Type::Float
        } else {
            Type::Int
        };
        let value = expression(K::Unwrap(Box::new(value)), inner, span);
        return Ok(if owner == "CUInt64" {
            let condition = compare(
                ast::BinaryOp::Ge,
                value.clone(),
                expression(K::Int(0), Type::Int, span),
                span,
            );
            checked_value(condition, value, result, span)
        } else {
            value
        });
    }
    let nominal = Type::Named(owner.into(), vec![]);
    let wrapped = expression(K::Wrap(Box::new(value.clone())), nominal, span);
    if owner == "CFloat32" {
        // Rounding occurs at the explicit CFloat32 conversion boundary, not later native calls.
        return internal("$ffi.float32", vec![value], result, span);
    }
    let (min, max) = match owner {
        "CInt8" => (i8::MIN as i64, i8::MAX as i64),
        "CInt16" => (i16::MIN as i64, i16::MAX as i64),
        "CInt32" => (i32::MIN as i64, i32::MAX as i64),
        "CUInt8" => (0, u8::MAX as i64),
        "CUInt16" => (0, u16::MAX as i64),
        "CUInt32" => (0, u32::MAX as i64),
        "CUInt64" => (0, i64::MAX),
        _ => return Err(Diagnostic::new(span, "unknown foreign scalar conversion")),
    };
    let low = compare(
        ast::BinaryOp::Ge,
        value.clone(),
        expression(K::Int(min), Type::Int, span),
        span,
    );
    let high = compare(
        ast::BinaryOp::Le,
        value,
        expression(K::Int(max), Type::Int, span),
        span,
    );
    let condition = compare(ast::BinaryOp::And, low, high, span);
    Ok(checked_value(condition, wrapped, result, span))
}
