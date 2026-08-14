use crate::ast::{
    AddressTerm, DataItem, ImportDeclaration, Instruction, MemoryDeclaration, MemoryValue, Operand,
    Program,
};
use crate::grammar::Token;
use crate::lexer::get_next_token;
use crate::parser::Parser;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub fn load_program(path: &Path) -> Result<Program, String> {
    let mut resolver = ImportResolver::new();
    resolver.load_root(path)
}

struct ImportResolver {
    modules: HashMap<PathBuf, ResolvedModule>,
    loading: Vec<PathBuf>,
    next_module_id: usize,
}

#[derive(Clone)]
struct ResolvedModule {
    program: Program,
    module_id: usize,
}

impl ImportResolver {
    fn new() -> Self {
        Self {
            modules: HashMap::new(),
            loading: Vec::new(),
            next_module_id: 0,
        }
    }

    fn load_root(&mut self, path: &Path) -> Result<Program, String> {
        let path = canonical_path(path)?;
        let mut program = parse_file(&path, true, &mut self.loading)?;
        self.resolve_imports_into_root(&mut program, &path)?;
        Ok(program)
    }

    fn resolve_imports_into_root(
        &mut self,
        program: &mut Program,
        source_path: &Path,
    ) -> Result<(), String> {
        let imports = std::mem::take(&mut program.imports);
        let base_dir = source_path.parent().unwrap_or_else(|| Path::new("."));
        let local_labels: HashSet<String> = program
            .labels
            .iter()
            .map(|label| label.name.clone())
            .collect();
        let mut imported_names = HashSet::new();
        let mut merged_modules = HashSet::new();

        for import in imports {
            let import_path = canonical_path(&base_dir.join(&import.path))?;
            let module = self.load_module(&import_path)?;
            validate_import(&module.program, &import, &local_labels, &mut imported_names)?;

            if merged_modules.insert(import_path) {
                merge_module(program, &module.program, module.module_id);
            }
        }

        Ok(())
    }

    fn load_module(&mut self, path: &Path) -> Result<ResolvedModule, String> {
        let path = canonical_path(path)?;
        if let Some(module) = self.modules.get(&path) {
            return Ok(module.clone());
        }

        let module_id = self.next_module_id;
        self.next_module_id += 1;

        let mut program = parse_file(&path, false, &mut self.loading)?;
        self.resolve_module_imports(&mut program, &path)?;
        let module = ResolvedModule { program, module_id };
        self.modules.insert(path, module.clone());

        Ok(module)
    }

    fn resolve_module_imports(
        &mut self,
        program: &mut Program,
        source_path: &Path,
    ) -> Result<(), String> {
        let imports = std::mem::take(&mut program.imports);
        let base_dir = source_path.parent().unwrap_or_else(|| Path::new("."));
        let local_labels: HashSet<String> = program
            .labels
            .iter()
            .map(|label| label.name.clone())
            .collect();
        let mut imported_names = HashSet::new();
        let mut merged_modules = HashSet::new();

        for import in imports {
            let import_path = canonical_path(&base_dir.join(&import.path))?;
            let module = self.load_module(&import_path)?;
            validate_import(&module.program, &import, &local_labels, &mut imported_names)?;

            if merged_modules.insert(import_path) {
                merge_module(program, &module.program, module.module_id);
            }
        }

        Ok(())
    }
}

fn parse_file(
    path: &Path,
    require_main: bool,
    loading: &mut Vec<PathBuf>,
) -> Result<Program, String> {
    let path = canonical_path(path)?;
    if loading.contains(&path) {
        return Err(format!("Import cycle involving {}", path.display()));
    }

    loading.push(path.clone());
    let source = fs::read_to_string(&path)
        .map_err(|error| format!("Failed to read {:?}: {error}", path.display().to_string()))?;
    let tokens = lex_source(&source)?;
    let mut parser = Parser::new(tokens);
    let program = if require_main {
        parser.parse_program()?
    } else {
        parser.parse_library()?
    };
    loading.pop();

    Ok(program)
}

fn validate_import(
    imported: &Program,
    import: &ImportDeclaration,
    local_labels: &HashSet<String>,
    imported_names: &mut HashSet<String>,
) -> Result<(), String> {
    let exports: HashSet<&str> = imported.exports.iter().map(String::as_str).collect();
    for name in &import.names {
        if !exports.contains(name.as_str()) {
            return Err(format!(
                "Function {name:?} is not exported by {:?}",
                import.path
            ));
        }

        if local_labels.contains(name) || !imported_names.insert(name.clone()) {
            return Err(format!(
                "Imported function {name:?} conflicts with an existing function"
            ));
        }
    }

    Ok(())
}

fn merge_module(program: &mut Program, imported: &Program, module_id: usize) {
    let symbol_map = build_symbol_map(imported, module_id);

    for declaration in &imported.data {
        let mut declaration = declaration.clone();
        declaration.name = rewrite_symbol_name(&declaration.name, &symbol_map);
        for item in &mut declaration.items {
            match item {
                DataItem::Addr { target } | DataItem::Label { name: target } => {
                    *target = rewrite_symbol_name(target, &symbol_map);
                }
                _ => {}
            }
        }
        program.data.push(declaration);
    }

    for declaration in &imported.memory {
        let mut declaration = declaration.clone();
        match &mut declaration {
            MemoryDeclaration::Scalar { name, .. }
            | MemoryDeclaration::FloatScalar { name, .. }
            | MemoryDeclaration::Buffer { name, .. }
            | MemoryDeclaration::Array { name, .. }
            | MemoryDeclaration::Repeat { name, .. } => {
                *name = rewrite_symbol_name(name, &symbol_map);
            }
        }
        rewrite_memory_declaration_symbols(&mut declaration, &symbol_map);
        program.memory.push(declaration);
    }

    for label in &imported.labels {
        let mut label = label.clone();
        label.name = rewrite_symbol_name(&label.name, &symbol_map);
        for instruction in &mut label.instructions {
            rewrite_instruction_symbols(instruction, &symbol_map);
        }
        program.labels.push(label);
    }
}

fn build_symbol_map(imported: &Program, module_id: usize) -> HashMap<String, String> {
    let exports: HashSet<&str> = imported.exports.iter().map(String::as_str).collect();
    let mut symbol_map = HashMap::new();

    for label in &imported.labels {
        if exports.contains(label.name.as_str()) {
            symbol_map.insert(label.name.clone(), label.name.clone());
        } else {
            symbol_map.insert(
                label.name.clone(),
                private_import_name(module_id, &label.name),
            );
        }
    }

    for declaration in &imported.data {
        symbol_map.insert(
            declaration.name.clone(),
            private_import_name(module_id, &declaration.name),
        );
        for item in &declaration.items {
            if let DataItem::Label { name } = item {
                symbol_map.insert(name.clone(), private_import_name(module_id, name));
            }
        }
    }

    for declaration in &imported.memory {
        let name = match declaration {
            MemoryDeclaration::Scalar { name, .. }
            | MemoryDeclaration::FloatScalar { name, .. }
            | MemoryDeclaration::Buffer { name, .. }
            | MemoryDeclaration::Array { name, .. }
            | MemoryDeclaration::Repeat { name, .. } => name,
        };
        symbol_map.insert(name.clone(), private_import_name(module_id, name));
    }

    symbol_map
}

fn rewrite_memory_declaration_symbols(
    declaration: &mut MemoryDeclaration,
    symbol_map: &HashMap<String, String>,
) {
    match declaration {
        MemoryDeclaration::Array { values, .. } => {
            for value in values {
                rewrite_memory_value_symbols(value, symbol_map);
            }
        }
        MemoryDeclaration::Repeat { value, .. } => rewrite_memory_value_symbols(value, symbol_map),
        _ => {}
    }
}

fn rewrite_memory_value_symbols(value: &mut MemoryValue, symbol_map: &HashMap<String, String>) {
    if let MemoryValue::Addr { target } = value {
        *target = rewrite_symbol_name(target, symbol_map);
    }
}

fn rewrite_instruction_symbols(
    instruction: &mut Instruction,
    symbol_map: &HashMap<String, String>,
) {
    match instruction {
        Instruction::Call { target } | Instruction::Jmp { target, .. } => {
            *target = rewrite_symbol_name(target, symbol_map);
        }
        Instruction::Label { name } => {
            *name = rewrite_symbol_name(name, symbol_map);
        }
        _ => {}
    }

    instruction.visit_operands_mut(|operand| rewrite_operand_symbols(operand, symbol_map));
}

fn rewrite_operand_symbols(operand: &mut Operand, symbol_map: &HashMap<String, String>) {
    match operand {
        Operand::AddressOf(address) | Operand::Dereference { address, .. } => {
            rewrite_address_term(&mut address.first, symbol_map);
            for (_, term) in &mut address.rest {
                rewrite_address_term(term, symbol_map);
            }
        }
        Operand::Converted { operand, .. } | Operand::Cast { operand, .. } => {
            rewrite_operand_symbols(operand, symbol_map)
        }
        Operand::Ident(name) | Operand::Pointer(name) | Operand::StringProperty { name, .. } => {
            *name = rewrite_symbol_name(name, symbol_map);
        }
        _ => {}
    }
}

fn rewrite_address_term(term: &mut AddressTerm, symbol_map: &HashMap<String, String>) {
    if let AddressTerm::Ident(name) = term {
        *name = rewrite_symbol_name(name, symbol_map);
    }
}

fn rewrite_symbol_name(name: &str, symbol_map: &HashMap<String, String>) -> String {
    if let Some(mapped) = symbol_map.get(name) {
        return mapped.clone();
    }

    for (old, new) in symbol_map {
        let prefix = format!(".L.{old}.");
        if let Some(suffix) = name.strip_prefix(&prefix) {
            return format!(".L.{new}.{suffix}");
        }
    }

    name.to_string()
}

fn private_import_name(module_id: usize, name: &str) -> String {
    format!("__import_{module_id}_{name}")
}

fn lex_source(source: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = source.chars().peekable();

    while let Some(token) = get_next_token(&mut chars)? {
        tokens.push(token);
    }

    Ok(tokens)
}

fn canonical_path(path: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(path)
        .map_err(|error| format!("Failed to read {:?}: {error}", path.display().to_string()))
}
