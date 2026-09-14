//! Typed collection callbacks retain their accumulator and partial output across safepoints.
use super::aggregates::mem;
use super::*;
use ir::Builtin as F;

impl Emitter<'_, '_> {
    pub(super) fn higher_order(
        &mut self,
        builtin: F,
        args: &[Expr],
        result: &Type,
        locals: &mut Locals,
        depth: usize,
        span: Span,
    ) -> Result<Type> {
        let arity = if builtin == F::ListFold {
            3
        } else if builtin == F::ListEnumerate {
            1
        } else {
            2
        };
        if args.len() != arity {
            return Err(invalid(span, "higher-order argument count mismatch"));
        }
        let mut values = Vec::new();
        for arg in args {
            let ty = self.expr(arg, locals, depth)?;
            let slot = self.temp(value_type(&ty, span)?, span)?;
            self.emit(I::LocalSet(slot));
            values.push((slot, ty));
        }
        if matches!(
            builtin,
            F::OptionMap | F::ResultMap | F::ResultAndThen | F::ResultUnwrapOrElse
        ) {
            return self.map_sum(builtin, &values, result, span);
        }
        let (input, Type::List(item)) = &values[0] else {
            return Err(invalid(span, "higher-order operation requires List"));
        };
        let input = *input;
        let callback = if builtin == F::ListFold {
            values.get(2)
        } else {
            values.get(1)
        };
        let len = self.temp(ValType::I32, span)?;
        self.emit(I::LocalGet(input));
        self.emit(I::I64Load(mem(8)));
        self.emit(I::I32WrapI64);
        self.emit(I::LocalSet(len));
        let output_item = match builtin {
            F::ListMap | F::ListFilter | F::ListEnumerate => match result {
                Type::List(ty) => Some(*ty.clone()),
                _ => return Err(invalid(span, "list callback result requires List")),
            },
            _ => None,
        };
        let output = if output_item.is_some() {
            Some(self.new_list(len, span)?)
        } else {
            None
        };
        let accumulator = if builtin == F::ListFold {
            Some(values[1].0)
        } else {
            None
        };
        let count = self.temp(ValType::I32, span)?;
        let index = self.temp(ValType::I32, span)?;
        self.emit(I::I32Const(0));
        self.emit(I::LocalSet(count));
        self.emit(I::I32Const(0));
        self.emit(I::LocalSet(index));
        let mark = self.temp(ValType::I32, span)?;
        self.emit(I::GlobalGet(0));
        self.emit(I::LocalSet(mark));
        self.emit(I::Block(block_type(result, span)?));
        self.emit(I::Block(BlockType::Empty));
        self.emit(I::Loop(BlockType::Empty));
        self.emit(I::LocalGet(index));
        self.emit(I::LocalGet(len));
        self.emit(I::I32GeU);
        self.emit(I::BrIf(1));
        self.dynamic_cell(input, index, item, span)?;
        let element = self.temp(value_type(item, span)?, span)?;
        self.emit(I::LocalSet(element));
        let produced_type = if builtin == F::ListEnumerate {
            let number = self.temp(ValType::I64, span)?;
            self.emit(I::LocalGet(index));
            self.emit(I::I64ExtendI32U);
            self.emit(I::LocalSet(number));
            let pair =
                self.allocate_aggregate(0, &[(number, Type::Int), (element, *item.clone())], span)?;
            self.emit(I::LocalGet(pair));
            Type::Tuple(vec![Type::Int, *item.clone()])
        } else if builtin == F::ListContains {
            self.equal_values(element, values[1].0, item, span)?;
            Type::Bool
        } else {
            let callback = callback.ok_or_else(|| invalid(span, "missing callback"))?;
            let mut arguments = Vec::new();
            if let Some(accumulator) = accumulator {
                arguments.push((accumulator, values[1].1.clone()));
            }
            arguments.push((element, *item.clone()));
            self.invoke_values(callback.0, &callback.1, &arguments, span)?
        };
        let produced = self.temp(value_type(&produced_type, span)?, span)?;
        self.emit(I::LocalSet(produced));
        if managed(&produced_type) {
            self.emit(I::LocalGet(produced));
            self.root_string(span)?;
            self.emit(I::Drop);
        }
        match builtin {
            F::ListMap | F::ListEnumerate => {
                let ty = output_item
                    .as_ref()
                    .ok_or_else(|| invalid(span, "missing output item"))?;
                expect(&produced_type, ty, span)?;
                self.store_dynamic(output.unwrap(), index, produced, ty, span)?;
            }
            F::ListFilter => {
                expect(&produced_type, &Type::Bool, span)?;
                expect(output_item.as_ref().unwrap(), item, span)?;
                self.emit(I::LocalGet(produced));
                self.emit(I::If(BlockType::Empty));
                self.store_dynamic(output.unwrap(), count, element, item, span)?;
                self.emit(I::LocalGet(count));
                self.emit(I::I32Const(1));
                self.emit(I::I32Add);
                self.emit(I::LocalSet(count));
                self.emit(I::End);
            }
            F::ListFold => {
                expect(&produced_type, result, span)?;
                expect(&values[1].1, result, span)?;
                self.emit(I::LocalGet(produced));
                self.emit(I::LocalSet(accumulator.unwrap()));
            }
            F::ListFind | F::ListAny | F::ListAll | F::ListContains => {
                expect(&produced_type, &Type::Bool, span)?;
                self.emit(I::LocalGet(produced));
                if builtin == F::ListAll {
                    self.emit(I::I32Eqz);
                }
                self.emit(I::If(BlockType::Empty));
                if builtin == F::ListFind {
                    expect(result, &Type::Option(item.clone()), span)?;
                    let some = self.allocate_aggregate(0, &[(element, *item.clone())], span)?;
                    self.emit(I::LocalGet(some));
                } else {
                    expect(result, &Type::Bool, span)?;
                    self.emit(I::I32Const(i32::from(builtin != F::ListAll)));
                }
                self.emit(I::Br(3));
                self.emit(I::End);
            }
            _ => return Err(invalid(span, "unsupported higher-order operation")),
        }
        self.emit(I::LocalGet(mark));
        self.emit(I::GlobalSet(0));
        if let Some(accumulator) = accumulator
            && managed(result)
        {
            self.emit(I::LocalGet(accumulator));
            self.root_string(span)?;
            self.emit(I::Drop);
        }
        self.emit(I::LocalGet(index));
        self.emit(I::I32Const(1));
        self.emit(I::I32Add);
        self.emit(I::LocalSet(index));
        self.emit(I::Br(0));
        self.emit(I::End);
        self.emit(I::End);
        if let Some(output) = output {
            if builtin == F::ListFilter {
                self.emit(I::LocalGet(output));
                self.emit(I::LocalGet(count));
                self.emit(I::I64ExtendI32U);
                self.emit(I::I64Store(mem(8)));
            }
            self.emit(I::LocalGet(output));
        } else if let Some(accumulator) = accumulator {
            self.emit(I::LocalGet(accumulator));
        } else if builtin == F::ListFind {
            let none = self.allocate_aggregate(1, &[], span)?;
            self.emit(I::LocalGet(none));
        } else {
            self.emit(I::I32Const(i32::from(builtin == F::ListAll)));
        }
        self.emit(I::End);
        self.emit(I::LocalGet(mark));
        self.emit(I::GlobalSet(0));
        Ok(result.clone())
    }

    fn map_sum(
        &mut self,
        builtin: F,
        values: &[(u32, Type)],
        result: &Type,
        span: Span,
    ) -> Result<Type> {
        let (input, input_ty) = &values[0];
        let (success, error) = match input_ty {
            Type::Option(item) if builtin == F::OptionMap => (*item.clone(), None),
            Type::Result(item, error) if builtin != F::OptionMap => {
                (*item.clone(), Some(*error.clone()))
            }
            _ => {
                return Err(invalid(
                    span,
                    "callback helper requires matching Option or Result",
                ));
            }
        };
        self.emit(I::LocalGet(*input));
        self.emit(I::I64Load(mem(8)));
        self.emit(I::I64Eqz);
        self.emit(I::If(block_type(result, span)?));
        self.load_cell(*input, 0, &success, span)?;
        let payload = self.temp(value_type(&success, span)?, span)?;
        self.emit(I::LocalSet(payload));
        if builtin == F::ResultUnwrapOrElse {
            expect(result, &success, span)?;
            self.emit(I::LocalGet(payload));
        } else {
            let ty = self.invoke_values(values[1].0, &values[1].1, &[(payload, success)], span)?;
            if managed(&ty) {
                self.root_string(span)?;
            }
            let value = self.temp(value_type(&ty, span)?, span)?;
            self.emit(I::LocalSet(value));
            if builtin == F::ResultAndThen {
                expect(&ty, result, span)?;
                self.emit(I::LocalGet(value));
            } else {
                let expected = if builtin == F::OptionMap {
                    Type::Option(Box::new(ty.clone()))
                } else {
                    Type::Result(Box::new(ty.clone()), Box::new(error.clone().unwrap()))
                };
                expect(&expected, result, span)?;
                let wrapped = self.allocate_aggregate(0, &[(value, ty)], span)?;
                self.emit(I::LocalGet(wrapped));
            }
        }
        self.emit(I::Else);
        if let Some(error) = error {
            self.load_cell(*input, 0, &error, span)?;
            let value = self.temp(value_type(&error, span)?, span)?;
            self.emit(I::LocalSet(value));
            if builtin == F::ResultUnwrapOrElse {
                let actual =
                    self.invoke_values(values[1].0, &values[1].1, &[(value, error)], span)?;
                expect(&actual, result, span)?;
            } else {
                let wrapped = self.allocate_aggregate(1, &[(value, error)], span)?;
                self.emit(I::LocalGet(wrapped));
            }
        } else {
            let none = self.allocate_aggregate(1, &[], span)?;
            self.emit(I::LocalGet(none));
        }
        self.emit(I::End);
        Ok(result.clone())
    }
}
