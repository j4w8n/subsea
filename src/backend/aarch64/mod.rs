use crate::ast::{BitwiseUnaryOp, CompareOp, ExprOp, MathOp};
use crate::backend::{BackendError, RuntimeEmitter};
use crate::ir;
use std::collections::HashMap;

mod registers;

pub use registers::is_register;
pub(crate) use registers::is_vector;

pub fn emit(program: &ir::Program) -> Result<String, BackendError> {
    let mut asm = String::new();
    emit_data(&mut asm, program)?;
    asm.push_str(".text\n.global _start\n\n");

    for label in &program.labels {
        let slots = stack_slots(&label.stack);
        let frame_size = stack_frame_size(&label.stack);
        asm.push_str(&format!(
            "{}:\n",
            if label.name == program.entry {
                "_start"
            } else {
                &label.name
            }
        ));
        if frame_size > 0 {
            asm.push_str(&format!("  sub sp, sp, #{frame_size}\n"));
        }
        for (index, instruction) in label.instructions.iter().enumerate() {
            emit_instruction(&mut asm, instruction, &slots, frame_size)
                .map_err(|message| BackendError::new(message).at(&label.name, index))?;
        }
        if frame_size > 0
            && !label
                .instructions
                .iter()
                .any(|instruction| matches!(instruction, ir::Instruction::Ret))
        {
            asm.push_str(&format!("  add sp, sp, #{frame_size}\n"));
        }
    }

    Ok(asm)
}

fn emit_data(asm: &mut String, program: &ir::Program) -> Result<(), String> {
    if !program.data.is_empty() {
        asm.push_str(".section .rodata\n");
        for declaration in &program.data {
            if let Some(align) = declaration.align {
                asm.push_str(&format!(".balign {align}\n"));
            }
            if declaration.export {
                asm.push_str(&format!(".global {}\n", declaration.name));
            }
            asm.push_str(&format!("{}:\n", declaration.name));
            for item in &declaration.items {
                match item {
                    ir::DataItem::Scalar { width, value } => {
                        asm.push_str(&format!("  {} {value}\n", data_directive(*width)?));
                    }
                    ir::DataItem::Address { target } => {
                        asm.push_str(&format!("  .quad {target}\n"))
                    }
                    ir::DataItem::Zero { count } => asm.push_str(&format!("  .zero {count}\n")),
                    ir::DataItem::Label { name } => asm.push_str(&format!("{name}:\n")),
                }
            }
        }
    }

    for memory in &program.memory {
        match memory {
            ir::MemoryDeclaration::Buffer { name, width, count } => {
                asm.push_str(".section .bss\n");
                asm.push_str(&format!(
                    "{name}:\n  .zero {}\n",
                    width_size(*width) * count
                ));
            }
            ir::MemoryDeclaration::Scalar { name, width, value } => {
                asm.push_str(".section .data\n");
                asm.push_str(&format!("{name}:\n  {} {value}\n", data_directive(*width)?));
            }
            ir::MemoryDeclaration::Array {
                name,
                width,
                values,
            } => {
                asm.push_str(".section .data\n");
                asm.push_str(&format!("{name}:\n"));
                for value in values {
                    emit_memory_value(asm, *width, value)?;
                }
            }
            ir::MemoryDeclaration::Repeat {
                name,
                width,
                count,
                value,
            } => {
                asm.push_str(".section .data\n");
                asm.push_str(&format!("{name}:\n"));
                for _ in 0..*count {
                    emit_memory_value(asm, *width, value)?;
                }
            }
            ir::MemoryDeclaration::FloatScalar { .. } => {
                return unsupported("floating-point static data");
            }
        }
    }
    Ok(())
}

fn emit_memory_value(
    asm: &mut String,
    width: crate::ast::MemoryWidth,
    value: &ir::MemoryValue,
) -> Result<(), String> {
    match value {
        ir::MemoryValue::Integer(value) => {
            asm.push_str(&format!("  {} {value}\n", data_directive(width)?))
        }
        ir::MemoryValue::Address { target } => asm.push_str(&format!("  .quad {target}\n")),
    }
    Ok(())
}

fn data_directive(width: crate::ast::MemoryWidth) -> Result<&'static str, String> {
    match width {
        crate::ast::MemoryWidth::I8 | crate::ast::MemoryWidth::U8 => Ok(".byte"),
        crate::ast::MemoryWidth::I16 | crate::ast::MemoryWidth::U16 => Ok(".hword"),
        crate::ast::MemoryWidth::I32 | crate::ast::MemoryWidth::U32 => Ok(".word"),
        crate::ast::MemoryWidth::I64
        | crate::ast::MemoryWidth::U64
        | crate::ast::MemoryWidth::Ptr => Ok(".quad"),
        _ => unsupported("floating-point data directive"),
    }
}

fn width_size(width: crate::ast::MemoryWidth) -> usize {
    match width {
        crate::ast::MemoryWidth::I8 | crate::ast::MemoryWidth::U8 => 1,
        crate::ast::MemoryWidth::I16 | crate::ast::MemoryWidth::U16 => 2,
        crate::ast::MemoryWidth::I32 | crate::ast::MemoryWidth::U32 => 4,
        _ => 8,
    }
}

fn emit_instruction(
    asm: &mut String,
    instruction: &ir::Instruction,
    slots: &HashMap<String, usize>,
    frame_size: usize,
) -> Result<(), String> {
    match instruction {
        ir::Instruction::Assign { dst, value } => emit_assignment(asm, dst, value, slots),
        ir::Instruction::PairAssign { dst, op, lhs, rhs } => {
            let (first, second) = match op {
                crate::ast::PairBinaryOp::Add => ("adds", "adc"),
                crate::ast::PairBinaryOp::Subtract => ("subs", "sbc"),
            };
            asm.push_str(&format!(
                "  {first} {}, {}, {}\n  {second} {}, {}, {}\n",
                dst.low, lhs.low, rhs.low, dst.high, lhs.high, rhs.high
            ));
            Ok(())
        }
        ir::Instruction::AssignIf {
            dst,
            value,
            condition,
        } => {
            let ir::Operand::TargetRegister(dst) = dst else {
                return unsupported("conditional assignment destination");
            };
            let ir::Value::Operand(value) = value else {
                return unsupported("conditional assignment value");
            };
            let ir::Operand::Immediate(value) = value else {
                return unsupported("conditional assignment value");
            };
            let skip = ".L.__subsea.aarch64.assign_if_skip";
            emit_condition_branch(asm, condition, skip, false, slots)?;
            asm.push_str(&format!("  mov {dst}, #{value}\n"));
            asm.push_str(&format!("{skip}:\n"));
            Ok(())
        }
        ir::Instruction::Call { target } => {
            match target {
                ir::ControlTarget::Label(target) => asm.push_str(&format!("  bl {target}\n")),
                ir::ControlTarget::Operand(ir::Operand::TargetRegister(register)) => {
                    asm.push_str(&format!("  blr {register}\n"));
                }
                _ => return unsupported("indirect calls"),
            }
            Ok(())
        }
        ir::Instruction::Exit { code } => AArch64RuntimeEmitter { slots }
            .emit_exit(asm, *code)
            .map_err(|error| error.message),
        ir::Instruction::Jmp { target, condition } => {
            let ir::ControlTarget::Label(target) = target else {
                return unsupported("indirect jumps");
            };
            if let Some(condition) = condition {
                emit_condition_branch(asm, condition, target, true, slots)
            } else {
                asm.push_str(&format!("  b {target}\n"));
                Ok(())
            }
        }
        ir::Instruction::Label { name } => {
            asm.push_str(&format!("{name}:\n"));
            Ok(())
        }
        ir::Instruction::Nop => {
            asm.push_str("  nop\n");
            Ok(())
        }
        ir::Instruction::Runtime(operation) => AArch64RuntimeEmitter { slots }
            .emit_runtime(asm, operation)
            .map_err(|error| error.message),
        ir::Instruction::Ret => {
            if frame_size > 0 {
                asm.push_str(&format!("  add sp, sp, #{frame_size}\n"));
            }
            asm.push_str("  ret\n");
            Ok(())
        }
        ir::Instruction::Stack { name, width, value } => {
            let dst = stack_operand(name, Some(*width), slots)?;
            emit_assignment(asm, &dst, &ir::Value::Operand(value.clone()), slots)
        }
        ir::Instruction::Const { .. } => Ok(()),
        ir::Instruction::StackString { name, value } => emit_stack_string(asm, name, value, slots),
        ir::Instruction::Push { src } => {
            emit_value(asm, "x16", src, slots)?;
            asm.push_str("  str x16, [sp, #-16]!\n");
            Ok(())
        }
        ir::Instruction::Pop { dst } => {
            let ir::Operand::TargetRegister(dst) = dst else {
                return unsupported("pop destination");
            };
            asm.push_str(&format!("  ldr {dst}, [sp], #16\n"));
            Ok(())
        }
    }
}

struct AArch64RuntimeEmitter<'a> {
    slots: &'a HashMap<String, usize>,
}

impl RuntimeEmitter for AArch64RuntimeEmitter<'_> {
    fn emit_runtime(
        &mut self,
        asm: &mut String,
        operation: &ir::RuntimeOperation,
    ) -> Result<(), BackendError> {
        emit_runtime_operation(asm, operation, self.slots).map_err(BackendError::from)
    }

    fn emit_exit(&mut self, asm: &mut String, code: u8) -> Result<(), BackendError> {
        asm.push_str(&format!("  mov x0, #{code}\n  mov x8, #93\n  svc #0\n"));
        Ok(())
    }

    fn emit_reserve(
        &mut self,
        asm: &mut String,
        dst: &ir::Operand,
        len: &ir::Operand,
    ) -> Result<(), BackendError> {
        emit_linux_reserve(asm, dst, len).map_err(BackendError::from)
    }
}

fn emit_runtime_operation(
    asm: &mut String,
    operation: &ir::RuntimeOperation,
    slots: &HashMap<String, usize>,
) -> Result<(), String> {
    match operation {
        ir::RuntimeOperation::Print { parts } => {
            for part in parts {
                match part {
                    ir::PrintPart::Literal(value) => emit_literal_write(asm, value),
                    ir::PrintPart::Binding(name) => {
                        let offset = *slots
                            .get(name)
                            .ok_or_else(|| format!("Unknown print binding {name:?}"))?;
                        asm.push_str(&format!(
                            "  mov x0, #1\n  ldr x1, [sp, #{offset}]\n  ldr x2, [sp, #{}]\n  mov x8, #64\n  svc #0\n",
                            offset + 8
                        ));
                    }
                    ir::PrintPart::Operand(operand) => {
                        emit_integer_print(
                            asm,
                            operand,
                            ir::PrintFormat::SignedDecimal(crate::ast::MemoryWidth::I64),
                            slots,
                        )?;
                    }
                    ir::PrintPart::FormattedOperand { format, operand } => {
                        emit_integer_print(asm, operand, *format, slots)?;
                    }
                }
            }
            Ok(())
        }
        ir::RuntimeOperation::Read {
            source: ir::ReadSource::Stdin,
            dst,
            len,
        } => {
            asm.push_str("  mov x0, #0\n");
            emit_address_or_value(asm, "x1", dst, &HashMap::new())?;
            emit_value(asm, "x2", len, &HashMap::new())?;
            asm.push_str("  mov x8, #63\n  svc #0\n");
            Ok(())
        }
        ir::RuntimeOperation::Release { ptr, len } => {
            emit_value(asm, "x0", ptr, &HashMap::new())?;
            emit_value(asm, "x1", len, &HashMap::new())?;
            asm.push_str("  mov x8, #215\n  svc #0\n");
            Ok(())
        }
    }
}

fn emit_literal_write(asm: &mut String, value: &str) {
    let label = format!(".L.__subsea.aarch64.string_{}", asm.len());
    let bytes = value
        .as_bytes()
        .iter()
        .map(|byte| byte.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    asm.push_str(".section .rodata\n");
    asm.push_str(&format!("{label}:\n  .byte {bytes}\n.text\n"));
    asm.push_str(&format!(
        "  mov x0, #1\n  adrp x1, {label}\n  add x1, x1, :lo12:{label}\n  mov x2, #{}\n  mov x8, #64\n  svc #0\n",
        value.len()
    ));
}

fn emit_integer_print(
    asm: &mut String,
    source: &ir::Operand,
    format: ir::PrintFormat,
    slots: &HashMap<String, usize>,
) -> Result<(), String> {
    let signed = matches!(format, ir::PrintFormat::SignedDecimal(_));
    let (base, prefix) = match format {
        ir::PrintFormat::SignedDecimal(_) | ir::PrintFormat::UnsignedDecimal(_) => (10, ""),
        ir::PrintFormat::Hex => (16, "0x"),
        ir::PrintFormat::Binary => (2, "0b"),
        ir::PrintFormat::Pointer => (16, "0x"),
        ir::PrintFormat::Infer => return unsupported("inferred runtime printing"),
    };
    emit_value(asm, "x16", source, slots)?;
    let id = asm.len();
    let loop_label = format!(".L.__subsea.aarch64.print_loop_{id}");
    let done_label = format!(".L.__subsea.aarch64.print_done_{id}");
    let zero_label = format!(".L.__subsea.aarch64.print_zero_{id}");
    let sign_label = format!(".L.__subsea.aarch64.print_sign_{id}");
    let buffer = format!(".L.__subsea.aarch64.print_buffer_{id}");
    asm.push_str(".section .bss\n");
    asm.push_str(&format!("{buffer}:\n  .zero 128\n.text\n"));
    asm.push_str(&format!(
        "  adrp x17, {buffer}\n  add x17, x17, :lo12:{buffer}\n  add x17, x17, #128\n  mov x18, #{base}\n  cbz x16, {zero_label}\n"
    ));
    if signed {
        asm.push_str(&format!("  tbnz x16, #63, {sign_label}\n"));
    }
    if base == 10 {
        asm.push_str(&format!(
            "{loop_label}:\n  udiv x19, x16, x18\n  msub x20, x19, x18, x16\n  add x20, x20, #48\n  strb w20, [x17, #-1]!\n  mov x16, x19\n  cbnz x16, {loop_label}\n  b {done_label}\n"
        ));
    } else {
        asm.push_str(&format!(
            "{loop_label}:\n  and x20, x16, #{}\n  cmp x20, #10\n  add x20, x20, #48\n  add x20, x20, #39, ge\n  strb w20, [x17, #-1]!\n  lsr x16, x16, #{}\n  cbnz x16, {loop_label}\n  b {done_label}\n",
            base - 1,
            if base == 16 { 4 } else { 1 }
        ));
    }
    if signed {
        asm.push_str(&format!(
            "{sign_label}:\n  neg x16, x16\n  bl {loop_label}\n  mov w20, #45\n  strb w20, [x17, #-1]!\n  b {done_label}\n"
        ));
    }
    asm.push_str(&format!(
        "{zero_label}:\n  mov w20, #48\n  strb w20, [x17, #-1]!\n{done_label}:\n"
    ));
    for byte in prefix.as_bytes().iter().rev() {
        asm.push_str(&format!("  mov w20, #{byte}\n  strb w20, [x17, #-1]!\n"));
    }
    asm.push_str(&format!(
        "  mov x0, #1\n  mov x1, x17\n  adrp x21, {buffer}\n  add x21, x21, :lo12:{buffer}\n  add x21, x21, #128\n  sub x2, x21, x1\n  mov x8, #64\n  svc #0\n"
    ));
    Ok(())
}

fn emit_linux_reserve(
    asm: &mut String,
    dst: &ir::Operand,
    len: &ir::Operand,
) -> Result<(), String> {
    emit_value(asm, "x0", len, &HashMap::new())?;
    asm.push_str(
        "  mov x1, #0\n  mov x2, #3\n  mov x3, #34\n  mov x4, #-1\n  mov x5, #0\n  mov x8, #222\n  svc #0\n",
    );
    if let ir::Operand::TargetRegister(register) = dst {
        if register != "x0" {
            asm.push_str(&format!("  mov {register}, x0\n"));
        }
        Ok(())
    } else {
        unsupported("memory reserve destination")
    }
}

fn emit_value(
    asm: &mut String,
    destination: &str,
    source: &ir::Operand,
    slots: &HashMap<String, usize>,
) -> Result<(), String> {
    match source {
        ir::Operand::Immediate(value) => {
            asm.push_str(&format!("  mov {destination}, #{value}\n"));
        }
        ir::Operand::TargetRegister(register) => {
            if register != destination {
                asm.push_str(&format!("  mov {destination}, {register}\n"));
            }
        }
        ir::Operand::Memory { address, .. } => {
            asm.push_str(&format!(
                "  ldr {destination}, {}\n",
                memory_address(address)?
            ));
        }
        ir::Operand::Name(name) => {
            let slot = stack_operand(name, None, slots)?;
            emit_value(asm, destination, &slot, slots)?;
        }
        ir::Operand::StringProperty { name, property } => {
            let offset = *slots
                .get(name)
                .ok_or_else(|| format!("Unknown string stack slot {name:?}"))?
                + if matches!(property, ir::StringProperty::Len) {
                    8
                } else {
                    0
                };
            asm.push_str(&format!("  ldr {destination}, [sp, #{offset}]\n"));
        }
        ir::Operand::Pointer(name) => {
            asm.push_str(&format!(
                "  adrp {destination}, {name}\n  add {destination}, {destination}, :lo12:{name}\n"
            ));
        }
        _ => return unsupported("runtime operand"),
    }
    Ok(())
}

fn emit_address_or_value(
    asm: &mut String,
    destination: &str,
    source: &ir::Operand,
    slots: &HashMap<String, usize>,
) -> Result<(), String> {
    match source {
        ir::Operand::Memory { address, .. } => emit_address(asm, destination, address),
        ir::Operand::Name(name) => {
            let slot = stack_operand(name, None, slots)?;
            let ir::Operand::Memory { address, .. } = slot else {
                unreachable!()
            };
            emit_address(asm, destination, &address)
        }
        _ => emit_value(asm, destination, source, slots),
    }
}

fn emit_address(asm: &mut String, destination: &str, address: &ir::Address) -> Result<(), String> {
    match &address.first {
        ir::AddressTerm::TargetRegister(register) => {
            if register != destination {
                asm.push_str(&format!("  mov {destination}, {register}\n"));
            }
        }
        ir::AddressTerm::Name(name) => {
            asm.push_str(&format!(
                "  adrp {destination}, {name}\n  add {destination}, {destination}, :lo12:{name}\n"
            ));
        }
        _ => return unsupported("runtime address"),
    }
    for (operator, term) in &address.rest {
        let ir::AddressOperator::Add = operator else {
            return unsupported("negative runtime address terms");
        };
        match term {
            ir::AddressTerm::Immediate(value) => {
                asm.push_str(&format!("  add {destination}, {destination}, #{value}\n"));
            }
            ir::AddressTerm::TargetRegister(register) => {
                asm.push_str(&format!("  add {destination}, {destination}, {register}\n"));
            }
            _ => return unsupported("runtime address term"),
        }
    }
    Ok(())
}

fn emit_assignment(
    asm: &mut String,
    dst: &ir::Operand,
    value: &ir::Value,
    slots: &HashMap<String, usize>,
) -> Result<(), String> {
    if let ir::Operand::Memory { address, width } = dst {
        let address = memory_address(address)?;
        let ir::Value::Operand(src) = value else {
            return unsupported("memory assignment value");
        };
        let src = operand(src, slots)?;
        if src.starts_with('#') {
            asm.push_str(&format!("  mov x16, {src}\n"));
            asm.push_str(&format!(
                "  str {}, {address}\n",
                narrow_register("x16", *width)
            ));
        } else {
            asm.push_str(&format!(
                "  str {}, {address}\n",
                narrow_register(&src, *width)
            ));
        }
        return Ok(());
    }

    let ir::Operand::TargetRegister(dst) = dst else {
        return unsupported("assignment destination");
    };

    match value {
        ir::Value::Operand(ir::Operand::Immediate(value)) => {
            asm.push_str(&format!("  mov {dst}, #{value}\n"));
        }
        ir::Value::Operand(ir::Operand::TargetRegister(src)) => {
            asm.push_str(&format!("  mov {dst}, {src}\n"));
        }
        ir::Value::Operand(ir::Operand::Memory { address, width }) => {
            asm.push_str(&format!(
                "  ldr {}, {}\n",
                narrow_register(dst, *width),
                memory_address(address)?
            ));
        }
        ir::Value::Binary { op, lhs, rhs } => {
            let lhs = operand(lhs, slots)?;
            let rhs = operand(rhs, slots)?;
            let opcode = integer_opcode(*op)?;
            asm.push_str(&format!("  {opcode} {dst}, {lhs}, {rhs}\n"));
        }
        ir::Value::BitwiseUnary {
            op,
            operand: source,
        } => {
            let operand = operand(source, slots)?;
            let opcode = bitwise_unary_opcode(*op);
            asm.push_str(&format!("  {opcode} {dst}, {operand}\n"));
        }
        ir::Value::Expression { op, lhs, rhs } => {
            emit_expression(asm, dst, op, lhs, rhs, slots)?;
        }
        ir::Value::FloatBinary {
            width,
            op,
            lhs,
            rhs,
        } => emit_float_binary(asm, dst, *width, *op, lhs, rhs, slots)?,
        ir::Value::IntrinsicCall { op, width, args } => {
            emit_intrinsic(asm, dst, *op, *width, args, slots)?;
        }
        ir::Value::Operand(ir::Operand::Cast { operand, width }) => {
            emit_cast(asm, dst, operand, *width, slots)?;
        }
        ir::Value::Operand(ir::Operand::Converted {
            operand,
            conversion,
        }) => {
            emit_value(asm, "x16", operand, slots)?;
            let opcode = match conversion {
                ir::WidthConversion::SignExtend => "sxtw",
                ir::WidthConversion::ZeroExtend => "uxtw",
            };
            asm.push_str(&format!("  {opcode} {dst}, w16\n"));
        }
        ir::Value::PlatformReserve { len } => AArch64RuntimeEmitter { slots }
            .emit_reserve(asm, &ir::Operand::TargetRegister(dst.clone()), len)
            .map_err(|error| error.message)?,
        _ => return unsupported("assignment value"),
    }
    Ok(())
}

fn emit_expression(
    asm: &mut String,
    destination: &str,
    op: &ExprOp,
    lhs: &ir::Value,
    rhs: &ir::Value,
    slots: &HashMap<String, usize>,
) -> Result<(), String> {
    emit_value_into_register(asm, "x16", lhs, slots)?;
    emit_value_into_register(asm, "x17", rhs, slots)?;
    match op {
        ExprOp::Math(op) => {
            let opcode = integer_opcode(*op)?;
            asm.push_str(&format!("  {opcode} x16, x16, x17\n"));
        }
        ExprOp::Divide { signed } => {
            asm.push_str(&format!(
                "  {} x16, x16, x17\n",
                if *signed { "sdiv" } else { "udiv" }
            ));
        }
        ExprOp::Modulo { signed } => {
            asm.push_str(&format!(
                "  {} x18, x16, x17\n  msub x16, x18, x17, x16\n",
                if *signed { "sdiv" } else { "udiv" }
            ));
        }
        ExprOp::Power => {
            let loop_label = format!(".L.__subsea.aarch64.power_loop_{}", asm.len());
            let done_label = format!(".L.__subsea.aarch64.power_done_{}", asm.len());
            asm.push_str(&format!(
                "  mov x18, #1\n{loop_label}:\n  cbz x17, {done_label}\n  mul x18, x18, x16\n  sub x17, x17, #1\n  b {loop_label}\n{done_label}:\n  mov x16, x18\n"
            ));
        }
    }
    if destination != "x16" {
        asm.push_str(&format!("  mov {destination}, x16\n"));
    }
    Ok(())
}

fn emit_value_into_register(
    asm: &mut String,
    destination: &str,
    value: &ir::Value,
    slots: &HashMap<String, usize>,
) -> Result<(), String> {
    match value {
        ir::Value::Operand(source) => emit_value(asm, destination, source, slots),
        ir::Value::Expression { op, lhs, rhs } => {
            emit_expression(asm, destination, op, lhs, rhs, slots)
        }
        _ => unsupported("arithmetic expression value"),
    }
}

fn emit_float_binary(
    asm: &mut String,
    destination: &str,
    width: crate::ast::MemoryWidth,
    op: crate::ast::FloatMathOp,
    lhs: &ir::Operand,
    rhs: &ir::Operand,
    slots: &HashMap<String, usize>,
) -> Result<(), String> {
    let suffix = match width {
        crate::ast::MemoryWidth::F32 => "s",
        crate::ast::MemoryWidth::F64 => "d",
        _ => return unsupported("floating-point width"),
    };
    let destination = float_register(destination, suffix)?;
    emit_float_operand(asm, "v16", suffix, lhs, slots)?;
    emit_float_operand(asm, "v17", suffix, rhs, slots)?;
    let opcode = match op {
        crate::ast::FloatMathOp::Add => "fadd",
        crate::ast::FloatMathOp::Divide => "fdiv",
        crate::ast::FloatMathOp::Multiply => "fmul",
        crate::ast::FloatMathOp::Subtract => "fsub",
    };
    asm.push_str(&format!(
        "  {opcode} {destination}, {suffix}16, {suffix}17\n"
    ));
    Ok(())
}

fn float_register(register: &str, suffix: &str) -> Result<String, String> {
    if let Some(index) = register.strip_prefix('v') {
        if index.parse::<u8>().is_ok_and(|index| index <= 31) {
            return Ok(format!("{suffix}{index}"));
        }
    }
    if (register.starts_with('s') && suffix == "s") || (register.starts_with('d') && suffix == "d")
    {
        return Ok(register.to_owned());
    }
    unsupported("floating-point destination register")
}

fn emit_float_operand(
    asm: &mut String,
    register: &str,
    suffix: &str,
    source: &ir::Operand,
    slots: &HashMap<String, usize>,
) -> Result<(), String> {
    let register = format!("{suffix}{}", register.trim_start_matches('v'));
    match source {
        ir::Operand::TargetRegister(source) => {
            let source = float_register(source, suffix)?;
            asm.push_str(&format!("  fmov {register}, {source}\n"));
        }
        ir::Operand::Memory { address, .. } => {
            asm.push_str(&format!("  ldr {register}, {}\n", memory_address(address)?));
        }
        ir::Operand::FloatLiteral(value) => {
            let label = format!(".L.__subsea.aarch64.float_{}", asm.len());
            let directive = if suffix == "s" { ".float" } else { ".double" };
            asm.push_str(&format!(
                ".section .rodata\n{label}:\n  {directive} {value}\n.text\n  adrp x16, {label}\n  add x16, x16, :lo12:{label}\n  ldr {register}, [x16]\n"
            ));
        }
        ir::Operand::Name(name) => {
            let slot = stack_operand(name, None, slots)?;
            emit_float_operand(asm, &register[..3], suffix, &slot, slots)?;
        }
        _ => return unsupported("floating-point operand"),
    }
    Ok(())
}

fn emit_intrinsic(
    asm: &mut String,
    destination: &str,
    op: crate::ast::IntrinsicOp,
    width: crate::ast::MemoryWidth,
    args: &[ir::Operand],
    slots: &HashMap<String, usize>,
) -> Result<(), String> {
    let suffix = match width {
        crate::ast::MemoryWidth::F32 => "s",
        crate::ast::MemoryWidth::F64 => "d",
        _ => return unsupported("non-floating intrinsic"),
    };
    let destination = float_register(destination, suffix)?;
    let first = args.first().ok_or("intrinsic requires an operand")?;
    emit_float_operand(asm, "v16", suffix, first, slots)?;
    if matches!(
        op,
        crate::ast::IntrinsicOp::Min | crate::ast::IntrinsicOp::Max
    ) {
        let second = args.get(1).ok_or("min/max requires two operands")?;
        emit_float_operand(asm, "v17", suffix, second, slots)?;
    }
    let opcode = match op {
        crate::ast::IntrinsicOp::Ceil => "frintp",
        crate::ast::IntrinsicOp::Floor => "frintm",
        crate::ast::IntrinsicOp::Max => "fmax",
        crate::ast::IntrinsicOp::Min => "fmin",
        crate::ast::IntrinsicOp::Round => "frintn",
        crate::ast::IntrinsicOp::Sqrt => "fsqrt",
        crate::ast::IntrinsicOp::Trunc => "frintz",
    };
    if matches!(
        op,
        crate::ast::IntrinsicOp::Min | crate::ast::IntrinsicOp::Max
    ) {
        asm.push_str(&format!(
            "  {opcode} {destination}, {suffix}16, {suffix}17\n"
        ));
    } else {
        asm.push_str(&format!("  {opcode} {destination}, {suffix}16\n"));
    }
    Ok(())
}

fn emit_cast(
    asm: &mut String,
    destination: &str,
    source: &ir::Operand,
    width: crate::ast::MemoryWidth,
    slots: &HashMap<String, usize>,
) -> Result<(), String> {
    let floating_destination = matches!(
        width,
        crate::ast::MemoryWidth::F32 | crate::ast::MemoryWidth::F64
    );
    if floating_destination {
        let suffix = if matches!(width, crate::ast::MemoryWidth::F32) {
            "s"
        } else {
            "d"
        };
        let destination = float_register(destination, suffix)?;
        emit_value(asm, "x16", source, slots)?;
        asm.push_str(&format!("  scvtf {destination}, x16\n"));
    } else {
        emit_float_operand(asm, "v16", "d", source, slots)?;
        let destination = if destination.starts_with('x') {
            destination.to_owned()
        } else {
            format!("x{destination}")
        };
        asm.push_str(&format!("  fcvtzs {destination}, d16\n"));
    }
    Ok(())
}

fn emit_condition_branch(
    asm: &mut String,
    condition: &ir::Condition,
    target: &str,
    branch_when_true: bool,
    slots: &HashMap<String, usize>,
) -> Result<(), String> {
    let ir::Condition::Compare { lhs, op, rhs } = condition else {
        return unsupported("bitwise conditions");
    };
    if is_float_operand(lhs) || is_float_operand(rhs) {
        let suffix = float_operand_suffix(lhs)
            .or_else(|| float_operand_suffix(rhs))
            .unwrap_or("d");
        emit_float_operand(asm, "v16", suffix, lhs, slots)?;
        emit_float_operand(asm, "v17", suffix, rhs, slots)?;
        asm.push_str(&format!("  fcmp {suffix}16, {suffix}17\n"));
        let opcode = float_compare_opcode(*op, branch_when_true)?;
        asm.push_str(&format!("  b.{opcode} {target}\n"));
        return Ok(());
    }
    let lhs = operand(lhs, slots)?;
    let rhs = operand(rhs, slots)?;
    asm.push_str(&format!("  cmp {lhs}, {rhs}\n"));
    let opcode = compare_opcode(*op, branch_when_true)?;
    asm.push_str(&format!("  {opcode} {target}\n"));
    Ok(())
}

fn is_float_operand(operand: &ir::Operand) -> bool {
    matches!(operand, ir::Operand::TargetRegister(name) if name.starts_with('v') || name.starts_with('s') || name.starts_with('d'))
}

fn float_operand_suffix(operand: &ir::Operand) -> Option<&'static str> {
    match operand {
        ir::Operand::TargetRegister(name) if name.starts_with('s') => Some("s"),
        ir::Operand::TargetRegister(name) if name.starts_with('d') => Some("d"),
        _ => None,
    }
}

fn float_compare_opcode(
    op: crate::ast::CompareOp,
    branch_when_true: bool,
) -> Result<&'static str, String> {
    let opcode = match op {
        crate::ast::CompareOp::Equal => "eq",
        crate::ast::CompareOp::NotEqual => "ne",
        crate::ast::CompareOp::SignedLess => "lt",
        crate::ast::CompareOp::SignedLessEqual => "le",
        crate::ast::CompareOp::SignedGreater => "gt",
        crate::ast::CompareOp::SignedGreaterEqual => "ge",
        _ => return unsupported("floating-point comparison"),
    };
    if branch_when_true {
        Ok(opcode)
    } else {
        Ok(match opcode {
            "eq" => "ne",
            "ne" => "eq",
            "lt" => "ge",
            "le" => "gt",
            "gt" => "le",
            "ge" => "lt",
            _ => unreachable!(),
        })
    }
}

fn operand(operand: &ir::Operand, slots: &HashMap<String, usize>) -> Result<String, String> {
    match operand {
        ir::Operand::Immediate(value) => Ok(format!("#{value}")),
        ir::Operand::TargetRegister(register) => Ok(register.clone()),
        ir::Operand::Memory { address, .. } => memory_address(address),
        ir::Operand::Name(name) => {
            let ir::Operand::Memory { address, .. } = stack_operand(name, None, slots)? else {
                unreachable!()
            };
            memory_address(&address)
        }
        ir::Operand::StringProperty { name, property } => {
            let offset = *slots
                .get(name)
                .ok_or_else(|| format!("Unknown string stack slot {name:?}"))?
                + if matches!(property, ir::StringProperty::Len) {
                    8
                } else {
                    0
                };
            Ok(format!("[sp, #{offset}]"))
        }
        ir::Operand::Pointer(name) => Ok(name.clone()),
        _ => unsupported("operand"),
    }
}

fn stack_slots(layout: &ir::StackLayout) -> HashMap<String, usize> {
    let mut offset = 0;
    let mut slots = HashMap::new();
    for slot in &layout.slots {
        let name = match slot {
            ir::StackSlot::Scalar { name, .. } | ir::StackSlot::String { name } => name,
        };
        slots.insert(name.clone(), offset);
        offset += if matches!(slot, ir::StackSlot::String { .. }) {
            16
        } else {
            8
        };
    }
    slots
}

fn stack_frame_size(layout: &ir::StackLayout) -> usize {
    let size = layout
        .slots
        .iter()
        .map(|slot| match slot {
            ir::StackSlot::Scalar { .. } => 8,
            ir::StackSlot::String { .. } => 16,
        })
        .sum::<usize>();
    size.div_ceil(16) * 16
}

fn stack_operand(
    name: &str,
    width: Option<crate::ast::MemoryWidth>,
    slots: &HashMap<String, usize>,
) -> Result<ir::Operand, String> {
    let offset = *slots
        .get(name)
        .ok_or_else(|| format!("Unknown stack slot {name:?}"))?;
    Ok(ir::Operand::Memory {
        address: ir::Address {
            first: ir::AddressTerm::TargetRegister(String::from("sp")),
            rest: if offset == 0 {
                Vec::new()
            } else {
                vec![(
                    ir::AddressOperator::Add,
                    ir::AddressTerm::Immediate(offset as i128),
                )]
            },
        },
        width,
    })
}

fn emit_stack_string(
    asm: &mut String,
    name: &str,
    value: &ir::StringInitializer,
    slots: &HashMap<String, usize>,
) -> Result<(), String> {
    let offset = *slots
        .get(name)
        .ok_or_else(|| format!("Unknown string stack slot {name:?}"))?;
    match value {
        ir::StringInitializer::Literal(value) => {
            let label = format!(".L.__subsea.aarch64.stack_string_{}", asm.len());
            let bytes = value
                .as_bytes()
                .iter()
                .map(|byte| byte.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            asm.push_str(".section .rodata\n");
            asm.push_str(&format!("{label}:\n  .byte {bytes}\n.text\n"));
            asm.push_str(&format!(
                "  adrp x16, {label}\n  add x16, x16, :lo12:{label}\n  str x16, [sp, #{offset}]\n  mov x16, #{}\n  str x16, [sp, #{}]\n",
                value.len(),
                offset + 8
            ));
        }
        ir::StringInitializer::Slice { ptr, len } => {
            emit_address_or_value(asm, "x16", ptr, slots)?;
            asm.push_str(&format!("  str x16, [sp, #{offset}]\n"));
            emit_value(asm, "x16", len, slots)?;
            asm.push_str(&format!("  str x16, [sp, #{}]\n", offset + 8));
        }
    }
    Ok(())
}

fn memory_address(address: &ir::Address) -> Result<String, String> {
    let mut terms = vec![address_term(&address.first)?];
    for (operator, term) in &address.rest {
        if *operator == ir::AddressOperator::Subtract {
            return unsupported("negative address terms");
        }
        terms.push(address_term(term)?);
    }
    let Some(base) = terms.first() else {
        return unsupported("empty address");
    };
    if terms.len() == 1 {
        return Ok(format!("[{base}]"));
    }
    Ok(format!("[{}, {}]", base, terms[1..].join(", ")))
}

fn address_term(term: &ir::AddressTerm) -> Result<String, String> {
    match term {
        ir::AddressTerm::TargetRegister(register) => Ok(register.clone()),
        ir::AddressTerm::Immediate(value) => Ok(format!("#{value}")),
        ir::AddressTerm::ScaledTargetRegister { register, scale } => {
            if !matches!(scale, 1 | 2 | 4 | 8) {
                return unsupported("unsupported address scale");
            }
            let shift = match scale {
                1 => 0,
                2 => 1,
                4 => 2,
                8 => 3,
                _ => unreachable!(),
            };
            Ok(format!("{register}, lsl #{shift}"))
        }
        ir::AddressTerm::Name(name) => Ok(name.clone()),
    }
}

fn narrow_register(register: &str, width: Option<crate::ast::MemoryWidth>) -> String {
    let narrow = width.is_some_and(|width| {
        matches!(
            width,
            crate::ast::MemoryWidth::I8
                | crate::ast::MemoryWidth::I16
                | crate::ast::MemoryWidth::I32
                | crate::ast::MemoryWidth::U8
                | crate::ast::MemoryWidth::U16
                | crate::ast::MemoryWidth::U32
        )
    });
    if narrow && register.starts_with('x') {
        format!("w{}", &register[1..])
    } else {
        register.to_owned()
    }
}

fn integer_opcode(op: MathOp) -> Result<&'static str, String> {
    match op {
        MathOp::Add => Ok("add"),
        MathOp::BitAnd => Ok("and"),
        MathOp::BitOr => Ok("orr"),
        MathOp::BitXor => Ok("eor"),
        MathOp::Subtract => Ok("sub"),
        MathOp::Multiply => Ok("mul"),
        MathOp::ShiftLeft => Ok("lsl"),
        MathOp::ShiftRightArithmetic => Ok("asr"),
        MathOp::ShiftRightLogical => Ok("lsr"),
        _ => unsupported("integer operation"),
    }
}

fn bitwise_unary_opcode(op: BitwiseUnaryOp) -> &'static str {
    match op {
        BitwiseUnaryOp::Not => "mvn",
    }
}

fn compare_opcode(op: CompareOp, branch_when_true: bool) -> Result<&'static str, String> {
    let opcode = match op {
        CompareOp::Equal => "eq",
        CompareOp::NotEqual => "ne",
        CompareOp::SignedLess => "lt",
        CompareOp::SignedLessEqual => "le",
        CompareOp::SignedGreater => "gt",
        CompareOp::SignedGreaterEqual => "ge",
        CompareOp::UnsignedLess => "lo",
        CompareOp::UnsignedLessEqual => "ls",
        CompareOp::UnsignedGreater => "hi",
        CompareOp::UnsignedGreaterEqual => "hs",
        _ => return unsupported("comparison"),
    };
    if branch_when_true {
        Ok(opcode)
    } else {
        Ok(match opcode {
            "eq" => "ne",
            "ne" => "eq",
            "lt" => "ge",
            "le" => "gt",
            "gt" => "le",
            "ge" => "lt",
            "lo" => "hs",
            "ls" => "hi",
            "hi" => "ls",
            "hs" => "lo",
            _ => unreachable!(),
        })
    }
}

fn unsupported<T>(feature: &str) -> Result<T, String> {
    Err(format!("AArch64 backend does not support {feature} yet"))
}
