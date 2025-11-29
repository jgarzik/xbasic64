//! Code generator - emits x86-64 assembly from AST

use crate::parser::*;
use std::collections::HashMap;

fn is_string_var(name: &str) -> bool {
    name.ends_with('$')
}

/// Metadata for array storage
struct ArrayInfo {
    ptr_offset: i32,       // stack offset where array pointer is stored
    dim_offsets: Vec<i32>, // stack offsets where dimension bounds are stored
}

pub struct CodeGen {
    output: String,
    vars: HashMap<String, i32>,         // variable name -> stack offset
    arrays: HashMap<String, ArrayInfo>, // array name -> array metadata
    stack_offset: i32,                  // current stack offset
    label_counter: u32,                 // for generating unique labels
    string_literals: Vec<String>,       // string constants
    data_items: Vec<Literal>,           // DATA values
    current_proc: Option<String>,       // current SUB/FUNCTION name
    proc_vars: HashMap<String, i32>,    // local variables for current proc
    gosub_used: bool,                   // whether GOSUB is used (need return stack)
    prefix: &'static str,               // symbol prefix ("_" on macOS, "" on Linux)
}

impl CodeGen {
    pub fn new() -> Self {
        // On macOS, symbols need underscore prefix
        #[cfg(target_os = "macos")]
        let prefix = "_";
        #[cfg(not(target_os = "macos"))]
        let prefix = "";

        CodeGen {
            output: String::new(),
            vars: HashMap::new(),
            arrays: HashMap::new(),
            stack_offset: 0,
            label_counter: 0,
            string_literals: Vec::new(),
            data_items: Vec::new(),
            current_proc: None,
            proc_vars: HashMap::new(),
            gosub_used: false,
            prefix,
        }
    }

    fn emit(&mut self, s: &str) {
        self.output.push_str(s);
        self.output.push('\n');
    }

    fn emit_label(&mut self, label: &str) {
        self.output.push_str(label);
        self.output.push_str(":\n");
    }

    fn new_label(&mut self, prefix: &str) -> String {
        let label = format!(".L{}_{}", prefix, self.label_counter);
        self.label_counter += 1;
        label
    }

    fn add_string_literal(&mut self, s: &str) -> usize {
        let idx = self.string_literals.len();
        self.string_literals.push(s.to_string());
        idx
    }

    fn get_var_offset(&mut self, name: &str) -> i32 {
        if self.current_proc.is_some() {
            // Check local variables first
            if let Some(&offset) = self.proc_vars.get(name) {
                return offset;
            }
        }

        if let Some(&offset) = self.vars.get(name) {
            return offset;
        }

        // Allocate new variable
        self.stack_offset -= 8;
        let offset = self.stack_offset;

        if self.current_proc.is_some() {
            self.proc_vars.insert(name.to_string(), offset);
        } else {
            self.vars.insert(name.to_string(), offset);
        }

        offset
    }

    pub fn generate(&mut self, program: &Program) -> String {
        // First pass: collect DATA statements and check for GOSUB
        for stmt in &program.statements {
            self.collect_data(stmt);
            self.check_gosub(stmt);
        }

        // Emit assembly header
        self.emit(".intel_syntax noprefix");
        self.emit(".text");
        let p = self.prefix;
        self.emit(&format!(".globl {}main", p));
        self.emit("");

        // Generate procedures first
        for stmt in &program.statements {
            if let Stmt::Sub { name, params, body } = stmt {
                self.gen_procedure(name, params, body, false);
            } else if let Stmt::Function { name, params, body } = stmt {
                self.gen_procedure(name, params, body, true);
            }
        }

        // Generate main
        self.emit_label(&format!("{}main", p));
        self.emit("    push rbp");
        self.emit("    mov rbp, rsp");

        // Reserve stack space (will patch later)
        self.emit("    sub rsp, 0         # STACK_RESERVE");

        // Initialize GOSUB return stack if needed
        if self.gosub_used {
            self.emit("    # Initialize GOSUB return stack");
            self.emit("    lea rax, [rbp - 8]");
            self.emit("    mov QWORD PTR [rip + _gosub_sp], rax");
        }

        // Generate main body
        for stmt in &program.statements {
            match stmt {
                Stmt::Sub { .. } | Stmt::Function { .. } => {}
                _ => self.gen_stmt(stmt),
            }
        }

        // Exit
        self.emit("    xor eax, eax");
        self.emit("    leave");
        self.emit("    ret");
        self.emit("");

        // Patch stack reserve
        let stack_size = (-self.stack_offset + 15) & !15; // Align to 16
        let old = "    sub rsp, 0         # STACK_RESERVE";
        let new = format!("    sub rsp, {}        # STACK_RESERVE", stack_size);
        self.output = self.output.replace(old, &new);

        // Emit data section
        self.emit_data_section();

        self.output.clone()
    }

    fn collect_data(&mut self, stmt: &Stmt) {
        if let Stmt::Data(values) = stmt {
            self.data_items.extend(values.clone());
        }
        // Recurse into nested statements
        match stmt {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                for s in then_branch {
                    self.collect_data(s);
                }
                if let Some(eb) = else_branch {
                    for s in eb {
                        self.collect_data(s);
                    }
                }
            }
            Stmt::For { body, .. } | Stmt::While { body, .. } | Stmt::DoLoop { body, .. } => {
                for s in body {
                    self.collect_data(s);
                }
            }
            Stmt::Sub { body, .. } | Stmt::Function { body, .. } => {
                for s in body {
                    self.collect_data(s);
                }
            }
            _ => {}
        }
    }

    fn check_gosub(&mut self, stmt: &Stmt) {
        if let Stmt::Gosub(_) = stmt {
            self.gosub_used = true;
        }
        // Recurse
        match stmt {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                for s in then_branch {
                    self.check_gosub(s);
                }
                if let Some(eb) = else_branch {
                    for s in eb {
                        self.check_gosub(s);
                    }
                }
            }
            Stmt::For { body, .. } | Stmt::While { body, .. } | Stmt::DoLoop { body, .. } => {
                for s in body {
                    self.check_gosub(s);
                }
            }
            Stmt::Sub { body, .. } | Stmt::Function { body, .. } => {
                for s in body {
                    self.check_gosub(s);
                }
            }
            _ => {}
        }
    }

    fn gen_procedure(&mut self, name: &str, params: &[String], body: &[Stmt], is_function: bool) {
        self.current_proc = Some(name.to_string());
        self.proc_vars.clear();
        let old_stack_offset = self.stack_offset;
        self.stack_offset = 0;

        // Procedure label
        self.emit_label(&format!("_proc_{}", name));
        self.emit("    push rbp");
        self.emit("    mov rbp, rsp");

        // Parameters are passed in registers (System V ABI)
        let int_regs = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"];
        for (i, param) in params.iter().enumerate() {
            self.stack_offset -= 8;
            self.proc_vars.insert(param.clone(), self.stack_offset);
            if i < int_regs.len() {
                self.emit(&format!(
                    "    mov QWORD PTR [rbp + {}], {}",
                    self.stack_offset, int_regs[i]
                ));
            }
        }

        // If function, allocate return value slot
        if is_function {
            self.stack_offset -= 8;
            self.proc_vars.insert(name.to_string(), self.stack_offset);
        }

        // Reserve stack space
        self.emit("    sub rsp, 64  # local vars"); // Simple fixed allocation

        // Generate body
        for stmt in body {
            self.gen_stmt(stmt);
        }

        // Return
        if is_function {
            let ret_offset = self.proc_vars[name];
            self.emit(&format!("    movsd xmm0, QWORD PTR [rbp + {}]", ret_offset));
        }

        self.emit("    leave");
        self.emit("    ret");
        self.emit("");

        self.current_proc = None;
        self.stack_offset = old_stack_offset;
    }

    fn gen_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Label(n) => {
                self.emit_label(&format!("_line_{}", n));
            }

            Stmt::Let {
                name,
                indices,
                value,
            } => {
                if indices.is_some() {
                    // Array assignment
                    self.gen_array_store(name, indices.as_ref().unwrap(), value);
                } else if is_string_var(name) {
                    self.gen_string_assign(name, value);
                } else {
                    self.gen_expr(value);
                    let offset = self.get_var_offset(name);
                    self.emit(&format!("    movsd QWORD PTR [rbp + {}], xmm0", offset));
                }
            }

            Stmt::Print { items, newline } => {
                for item in items {
                    match item {
                        PrintItem::Expr(expr) => {
                            self.gen_print_expr(expr);
                        }
                        PrintItem::Tab => {
                            self.emit("    mov rdi, 9  # tab");
                            self.emit("    call _rt_print_char");
                        }
                        PrintItem::Empty => {}
                    }
                }
                if *newline {
                    self.emit("    call _rt_print_newline");
                }
            }

            Stmt::Input { prompt, vars } => {
                if let Some(pstr) = prompt {
                    let idx = self.add_string_literal(pstr);
                    self.emit(&format!("    lea rdi, [rip + _str_{}]", idx));
                    self.emit(&format!("    mov rsi, {}", pstr.len()));
                    self.emit("    call _rt_print_string");
                }
                for var in vars {
                    if is_string_var(var) {
                        self.emit("    call _rt_input_string");
                        let offset = self.get_var_offset(var);
                        self.emit(&format!("    mov QWORD PTR [rbp + {}], rax", offset));
                        self.emit(&format!("    mov QWORD PTR [rbp + {}], rdx", offset - 8));
                    } else {
                        self.emit("    call _rt_input_number");
                        let offset = self.get_var_offset(var);
                        self.emit(&format!("    movsd QWORD PTR [rbp + {}], xmm0", offset));
                    }
                }
            }

            Stmt::LineInput { prompt, var } => {
                if let Some(pstr) = prompt {
                    let idx = self.add_string_literal(pstr);
                    self.emit(&format!("    lea rdi, [rip + _str_{}]", idx));
                    self.emit(&format!("    mov rsi, {}", pstr.len()));
                    self.emit("    call _rt_print_string");
                }
                self.emit("    call _rt_input_string");
                let offset = self.get_var_offset(var);
                self.emit(&format!("    mov QWORD PTR [rbp + {}], rax", offset));
                self.emit(&format!("    mov QWORD PTR [rbp + {}], rdx", offset - 8));
            }

            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let else_label = self.new_label("else");
                let end_label = self.new_label("endif");

                self.gen_expr(condition);
                // Compare with 0
                self.emit("    xorpd xmm1, xmm1");
                self.emit("    ucomisd xmm0, xmm1");
                self.emit(&format!("    je {}", else_label));

                for s in then_branch {
                    self.gen_stmt(s);
                }
                self.emit(&format!("    jmp {}", end_label));

                self.emit_label(&else_label);
                if let Some(eb) = else_branch {
                    for s in eb {
                        self.gen_stmt(s);
                    }
                }

                self.emit_label(&end_label);
            }

            Stmt::For {
                var,
                start,
                end,
                step,
                body,
            } => {
                let start_label = self.new_label("for");
                let end_label = self.new_label("endfor");
                let var_offset = self.get_var_offset(var);

                // Initialize loop variable
                self.gen_expr(start);
                self.emit(&format!("    movsd QWORD PTR [rbp + {}], xmm0", var_offset));

                // Store end value
                self.stack_offset -= 8;
                let end_offset = self.stack_offset;
                self.gen_expr(end);
                self.emit(&format!("    movsd QWORD PTR [rbp + {}], xmm0", end_offset));

                // Store step value
                self.stack_offset -= 8;
                let step_offset = self.stack_offset;
                if let Some(s) = step {
                    self.gen_expr(s);
                } else {
                    self.emit("    mov rax, 0x3FF0000000000000  # 1.0");
                    self.emit("    movq xmm0, rax");
                }
                self.emit(&format!(
                    "    movsd QWORD PTR [rbp + {}], xmm0",
                    step_offset
                ));

                self.emit_label(&start_label);

                // Check condition (var > end for positive step, var < end for negative)
                self.emit(&format!("    movsd xmm0, QWORD PTR [rbp + {}]", var_offset));
                self.emit(&format!("    movsd xmm1, QWORD PTR [rbp + {}]", end_offset));
                self.emit(&format!(
                    "    movsd xmm2, QWORD PTR [rbp + {}]",
                    step_offset
                ));
                self.emit("    xorpd xmm3, xmm3");
                self.emit("    ucomisd xmm2, xmm3");
                self.emit(&format!("    jb .Lfor_neg_{}", self.label_counter));

                // Positive step: exit if var > end
                self.emit("    ucomisd xmm0, xmm1");
                self.emit(&format!("    ja {}", end_label));
                self.emit(&format!("    jmp .Lfor_body_{}", self.label_counter));

                // Negative step: exit if var < end
                self.emit_label(&format!(".Lfor_neg_{}", self.label_counter));
                self.emit("    ucomisd xmm0, xmm1");
                self.emit(&format!("    jb {}", end_label));

                self.emit_label(&format!(".Lfor_body_{}", self.label_counter));
                self.label_counter += 1;

                // Body
                for s in body {
                    self.gen_stmt(s);
                }

                // Increment
                self.emit(&format!("    movsd xmm0, QWORD PTR [rbp + {}]", var_offset));
                self.emit(&format!(
                    "    addsd xmm0, QWORD PTR [rbp + {}]",
                    step_offset
                ));
                self.emit(&format!("    movsd QWORD PTR [rbp + {}], xmm0", var_offset));
                self.emit(&format!("    jmp {}", start_label));

                self.emit_label(&end_label);
            }

            Stmt::While { condition, body } => {
                let start_label = self.new_label("while");
                let end_label = self.new_label("endwhile");

                self.emit_label(&start_label);
                self.gen_expr(condition);
                self.emit("    xorpd xmm1, xmm1");
                self.emit("    ucomisd xmm0, xmm1");
                self.emit(&format!("    je {}", end_label));

                for s in body {
                    self.gen_stmt(s);
                }
                self.emit(&format!("    jmp {}", start_label));

                self.emit_label(&end_label);
            }

            Stmt::DoLoop {
                condition,
                cond_at_start,
                is_until,
                body,
            } => {
                let start_label = self.new_label("do");
                let end_label = self.new_label("enddo");

                self.emit_label(&start_label);

                if *cond_at_start {
                    if let Some(cond) = condition {
                        self.gen_expr(cond);
                        self.emit("    xorpd xmm1, xmm1");
                        self.emit("    ucomisd xmm0, xmm1");
                        if *is_until {
                            self.emit(&format!("    jne {}", end_label));
                        } else {
                            self.emit(&format!("    je {}", end_label));
                        }
                    }
                }

                for s in body {
                    self.gen_stmt(s);
                }

                if !*cond_at_start {
                    if let Some(cond) = condition {
                        self.gen_expr(cond);
                        self.emit("    xorpd xmm1, xmm1");
                        self.emit("    ucomisd xmm0, xmm1");
                        if *is_until {
                            self.emit(&format!("    je {}", start_label));
                        } else {
                            self.emit(&format!("    jne {}", start_label));
                        }
                    } else {
                        self.emit(&format!("    jmp {}", start_label));
                    }
                } else {
                    self.emit(&format!("    jmp {}", start_label));
                }

                self.emit_label(&end_label);
            }

            Stmt::Goto(target) => {
                let label = match target {
                    GotoTarget::Line(n) => format!("_line_{}", n),
                    GotoTarget::Label(s) => format!("_label_{}", s),
                };
                self.emit(&format!("    jmp {}", label));
            }

            Stmt::Gosub(target) => {
                let label = match target {
                    GotoTarget::Line(n) => format!("_line_{}", n),
                    GotoTarget::Label(s) => format!("_label_{}", s),
                };
                let ret_label = self.new_label("gosub_ret");
                // Push return address to GOSUB stack
                self.emit(&format!("    lea rax, [rip + {}]", ret_label));
                self.emit("    mov rdi, QWORD PTR [rip + _gosub_sp]");
                self.emit("    sub rdi, 8");
                self.emit("    mov QWORD PTR [rdi], rax");
                self.emit("    mov QWORD PTR [rip + _gosub_sp], rdi");
                self.emit(&format!("    jmp {}", label));
                self.emit_label(&ret_label);
            }

            Stmt::Return => {
                // Pop return address from GOSUB stack and jump
                self.emit("    mov rdi, QWORD PTR [rip + _gosub_sp]");
                self.emit("    mov rax, QWORD PTR [rdi]");
                self.emit("    add rdi, 8");
                self.emit("    mov QWORD PTR [rip + _gosub_sp], rdi");
                self.emit("    jmp rax");
            }

            Stmt::OnGoto { expr, targets } => {
                self.gen_expr(expr);
                // Convert to integer
                self.emit("    cvttsd2si rax, xmm0");
                // Create jump table
                for (i, target) in targets.iter().enumerate() {
                    let label = match target {
                        GotoTarget::Line(n) => format!("_line_{}", n),
                        GotoTarget::Label(s) => format!("_label_{}", s),
                    };
                    self.emit(&format!("    cmp rax, {}", i + 1));
                    self.emit(&format!("    je {}", label));
                }
            }

            Stmt::Dim { arrays } => {
                for arr in arrays {
                    self.gen_dim_array(arr);
                }
            }

            Stmt::Sub { .. } | Stmt::Function { .. } => {
                // Already handled in first pass
            }

            Stmt::Call { name, args } => {
                self.gen_call(name, args);
            }

            Stmt::Data(_) => {
                // Data already collected in first pass
            }

            Stmt::Read(vars) => {
                for var in vars {
                    if is_string_var(var) {
                        self.emit("    call _rt_read_string");
                        let offset = self.get_var_offset(var);
                        self.emit(&format!("    mov QWORD PTR [rbp + {}], rax", offset));
                    } else {
                        self.emit("    call _rt_read_number");
                        let offset = self.get_var_offset(var);
                        self.emit(&format!("    movsd QWORD PTR [rbp + {}], xmm0", offset));
                    }
                }
            }

            Stmt::Restore(target) => {
                let idx = if let Some(_t) = target {
                    // TODO: find DATA line index
                    0
                } else {
                    0
                };
                self.emit(&format!("    mov rdi, {}", idx));
                self.emit("    call _rt_restore");
            }

            Stmt::Cls => {
                self.emit("    call _rt_cls");
            }

            Stmt::End | Stmt::Stop => {
                self.emit("    xor eax, eax");
                self.emit("    leave");
                self.emit("    ret");
            }

            Stmt::Open {
                filename,
                mode,
                file_num,
            } => {
                // Generate filename string (ptr in rax, len in rdx)
                self.gen_expr(filename);
                self.emit("    mov rdi, rax  # filename ptr");
                self.emit("    mov rsi, rdx  # filename len");
                let mode_num = match mode {
                    FileMode::Input => 0,
                    FileMode::Output => 1,
                    FileMode::Append => 2,
                };
                self.emit(&format!("    mov rdx, {}  # mode", mode_num));
                self.emit(&format!("    mov rcx, {}  # file number", file_num));
                self.emit("    call _rt_file_open");
            }

            Stmt::Close { file_num } => {
                self.emit(&format!("    mov rdi, {}", file_num));
                self.emit("    call _rt_file_close");
            }

            Stmt::PrintFile {
                file_num,
                items,
                newline,
            } => {
                for item in items {
                    match item {
                        PrintItem::Expr(expr) => {
                            self.gen_print_expr_to_file(expr, *file_num);
                        }
                        PrintItem::Tab => {
                            self.emit(&format!("    mov rdi, {}", file_num));
                            self.emit("    mov rsi, 9  # tab");
                            self.emit("    call _rt_file_print_char");
                        }
                        PrintItem::Empty => {}
                    }
                }
                if *newline {
                    self.emit(&format!("    mov rdi, {}", file_num));
                    self.emit("    call _rt_file_print_newline");
                }
            }

            Stmt::InputFile { file_num, vars } => {
                for var in vars {
                    if is_string_var(var) {
                        self.emit(&format!("    mov rdi, {}", file_num));
                        self.emit("    call _rt_file_input_string");
                        let offset = self.get_var_offset(var);
                        self.emit(&format!("    mov QWORD PTR [rbp + {}], rax", offset));
                        self.emit(&format!("    mov QWORD PTR [rbp + {}], rdx", offset - 8));
                    } else {
                        self.emit(&format!("    mov rdi, {}", file_num));
                        self.emit("    call _rt_file_input_number");
                        let offset = self.get_var_offset(var);
                        self.emit(&format!("    movsd QWORD PTR [rbp + {}], xmm0", offset));
                    }
                }
            }
        }
    }

    fn gen_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Literal(lit) => {
                match lit {
                    Literal::Integer(n) => {
                        // Load as float
                        let bits = (*n as f64).to_bits();
                        self.emit(&format!("    mov rax, 0x{:X}", bits));
                        self.emit("    movq xmm0, rax");
                    }
                    Literal::Float(f) => {
                        let bits = f.to_bits();
                        self.emit(&format!("    mov rax, 0x{:X}", bits));
                        self.emit("    movq xmm0, rax");
                    }
                    Literal::String(s) => {
                        let idx = self.add_string_literal(s);
                        self.emit(&format!("    lea rax, [rip + _str_{}]", idx));
                        self.emit(&format!("    mov rdx, {}", s.len()));
                    }
                }
            }

            Expr::Variable(name) => {
                let offset = self.get_var_offset(name);
                if is_string_var(name) {
                    self.emit(&format!("    mov rax, QWORD PTR [rbp + {}]", offset));
                    self.emit(&format!("    mov rdx, QWORD PTR [rbp + {}]", offset - 8));
                } else {
                    self.emit(&format!("    movsd xmm0, QWORD PTR [rbp + {}]", offset));
                }
            }

            Expr::ArrayAccess { name, indices } => {
                self.gen_array_load(name, indices);
            }

            Expr::Unary { op, operand } => {
                self.gen_expr(operand);
                match op {
                    UnaryOp::Neg => {
                        // Negate by XORing sign bit
                        self.emit("    mov rax, 0x8000000000000000");
                        self.emit("    movq xmm1, rax");
                        self.emit("    xorpd xmm0, xmm1");
                    }
                    UnaryOp::Not => {
                        // NOT: if 0 then -1, else 0
                        self.emit("    xorpd xmm1, xmm1");
                        self.emit("    ucomisd xmm0, xmm1");
                        self.emit("    sete al");
                        self.emit("    movzx eax, al");
                        self.emit("    neg eax");
                        self.emit("    cvtsi2sd xmm0, eax");
                    }
                }
            }

            Expr::Binary { op, left, right } => {
                // Evaluate left, push, evaluate right, pop, compute
                self.gen_expr(left);
                self.emit("    sub rsp, 8");
                self.emit("    movsd QWORD PTR [rsp], xmm0");
                self.gen_expr(right);
                self.emit("    movsd xmm1, xmm0");
                self.emit("    movsd xmm0, QWORD PTR [rsp]");
                self.emit("    add rsp, 8");

                match op {
                    BinaryOp::Add => self.emit("    addsd xmm0, xmm1"),
                    BinaryOp::Sub => self.emit("    subsd xmm0, xmm1"),
                    BinaryOp::Mul => self.emit("    mulsd xmm0, xmm1"),
                    BinaryOp::Div => self.emit("    divsd xmm0, xmm1"),
                    BinaryOp::IntDiv => {
                        self.emit("    divsd xmm0, xmm1");
                        self.emit("    roundsd xmm0, xmm0, 3"); // truncate
                    }
                    BinaryOp::Mod => {
                        // a MOD b = a - INT(a/b) * b
                        self.emit("    movsd xmm2, xmm0"); // save a
                        self.emit("    divsd xmm0, xmm1"); // a/b
                        self.emit("    roundsd xmm0, xmm0, 3"); // INT(a/b)
                        self.emit("    mulsd xmm0, xmm1"); // INT(a/b) * b
                        self.emit("    subsd xmm2, xmm0"); // a - INT(a/b) * b
                        self.emit("    movsd xmm0, xmm2");
                    }
                    BinaryOp::Pow => {
                        // Call pow function (libc)
                        self.emit(&format!("    call {}pow", self.prefix));
                    }
                    BinaryOp::Eq => {
                        self.emit("    ucomisd xmm0, xmm1");
                        self.emit("    sete al");
                        self.emit("    movzx eax, al");
                        self.emit("    neg eax");
                        self.emit("    cvtsi2sd xmm0, eax");
                    }
                    BinaryOp::Ne => {
                        self.emit("    ucomisd xmm0, xmm1");
                        self.emit("    setne al");
                        self.emit("    movzx eax, al");
                        self.emit("    neg eax");
                        self.emit("    cvtsi2sd xmm0, eax");
                    }
                    BinaryOp::Lt => {
                        self.emit("    ucomisd xmm0, xmm1");
                        self.emit("    setb al");
                        self.emit("    movzx eax, al");
                        self.emit("    neg eax");
                        self.emit("    cvtsi2sd xmm0, eax");
                    }
                    BinaryOp::Gt => {
                        self.emit("    ucomisd xmm0, xmm1");
                        self.emit("    seta al");
                        self.emit("    movzx eax, al");
                        self.emit("    neg eax");
                        self.emit("    cvtsi2sd xmm0, eax");
                    }
                    BinaryOp::Le => {
                        self.emit("    ucomisd xmm0, xmm1");
                        self.emit("    setbe al");
                        self.emit("    movzx eax, al");
                        self.emit("    neg eax");
                        self.emit("    cvtsi2sd xmm0, eax");
                    }
                    BinaryOp::Ge => {
                        self.emit("    ucomisd xmm0, xmm1");
                        self.emit("    setae al");
                        self.emit("    movzx eax, al");
                        self.emit("    neg eax");
                        self.emit("    cvtsi2sd xmm0, eax");
                    }
                    BinaryOp::And => {
                        // Bitwise AND of integer values
                        self.emit("    cvttsd2si rax, xmm0");
                        self.emit("    cvttsd2si rcx, xmm1");
                        self.emit("    and rax, rcx");
                        self.emit("    cvtsi2sd xmm0, rax");
                    }
                    BinaryOp::Or => {
                        self.emit("    cvttsd2si rax, xmm0");
                        self.emit("    cvttsd2si rcx, xmm1");
                        self.emit("    or rax, rcx");
                        self.emit("    cvtsi2sd xmm0, rax");
                    }
                    BinaryOp::Xor => {
                        self.emit("    cvttsd2si rax, xmm0");
                        self.emit("    cvttsd2si rcx, xmm1");
                        self.emit("    xor rax, rcx");
                        self.emit("    cvtsi2sd xmm0, rax");
                    }
                }
            }

            Expr::FnCall { name, args } => {
                self.gen_fn_call(name, args);
            }
        }
    }

    fn gen_print_expr(&mut self, expr: &Expr) {
        // Check if string expression
        if let Expr::Literal(Literal::String(s)) = expr {
            let idx = self.add_string_literal(s);
            self.emit(&format!("    lea rdi, [rip + _str_{}]", idx));
            self.emit(&format!("    mov rsi, {}", s.len()));
            self.emit("    call _rt_print_string");
        } else if let Expr::Variable(name) = expr {
            if is_string_var(name) {
                let offset = self.get_var_offset(name);
                self.emit(&format!("    mov rdi, QWORD PTR [rbp + {}]", offset));
                self.emit(&format!("    mov rsi, QWORD PTR [rbp + {}]", offset - 8));
                self.emit("    call _rt_print_string");
            } else {
                self.gen_expr(expr);
                self.emit("    call _rt_print_float");
            }
        } else {
            // Assume numeric
            self.gen_expr(expr);
            self.emit("    call _rt_print_float");
        }
    }

    fn gen_print_expr_to_file(&mut self, expr: &Expr, file_num: i32) {
        // Check if string expression
        if let Expr::Literal(Literal::String(s)) = expr {
            let idx = self.add_string_literal(s);
            self.emit(&format!("    mov rdi, {}", file_num));
            self.emit(&format!("    lea rsi, [rip + _str_{}]", idx));
            self.emit(&format!("    mov rdx, {}", s.len()));
            self.emit("    call _rt_file_print_string");
        } else if let Expr::Variable(name) = expr {
            if is_string_var(name) {
                let offset = self.get_var_offset(name);
                self.emit(&format!("    mov rdi, {}", file_num));
                self.emit(&format!("    mov rsi, QWORD PTR [rbp + {}]", offset));
                self.emit(&format!("    mov rdx, QWORD PTR [rbp + {}]", offset - 8));
                self.emit("    call _rt_file_print_string");
            } else {
                self.gen_expr(expr);
                self.emit(&format!("    mov rdi, {}", file_num));
                self.emit("    call _rt_file_print_float");
            }
        } else {
            // Assume numeric
            self.gen_expr(expr);
            self.emit(&format!("    mov rdi, {}", file_num));
            self.emit("    call _rt_file_print_float");
        }
    }

    fn gen_fn_call(&mut self, name: &str, args: &[Expr]) {
        let upper_name = name.to_uppercase();

        // Check for built-in functions
        match upper_name.as_str() {
            "ABS" => {
                self.gen_expr(&args[0]);
                self.emit("    mov rax, 0x7FFFFFFFFFFFFFFF");
                self.emit("    movq xmm1, rax");
                self.emit("    andpd xmm0, xmm1");
            }
            "INT" => {
                self.gen_expr(&args[0]);
                self.emit("    roundsd xmm0, xmm0, 1"); // floor
            }
            "FIX" => {
                self.gen_expr(&args[0]);
                self.emit("    roundsd xmm0, xmm0, 3"); // truncate
            }
            "SQR" => {
                self.gen_expr(&args[0]);
                self.emit("    sqrtsd xmm0, xmm0");
            }
            "SIN" => {
                self.gen_expr(&args[0]);
                self.emit(&format!("    call {}sin", self.prefix));
            }
            "COS" => {
                self.gen_expr(&args[0]);
                self.emit(&format!("    call {}cos", self.prefix));
            }
            "TAN" => {
                self.gen_expr(&args[0]);
                self.emit(&format!("    call {}tan", self.prefix));
            }
            "ATN" => {
                self.gen_expr(&args[0]);
                self.emit(&format!("    call {}atan", self.prefix));
            }
            "EXP" => {
                self.gen_expr(&args[0]);
                self.emit(&format!("    call {}exp", self.prefix));
            }
            "LOG" => {
                self.gen_expr(&args[0]);
                self.emit(&format!("    call {}log", self.prefix));
            }
            "SGN" => {
                self.gen_expr(&args[0]);
                self.emit("    xorpd xmm1, xmm1");
                self.emit("    ucomisd xmm0, xmm1");
                self.emit("    seta al");
                self.emit("    movzx eax, al");
                self.emit("    setb cl");
                self.emit("    movzx ecx, cl");
                self.emit("    sub eax, ecx");
                self.emit("    cvtsi2sd xmm0, eax");
            }
            "RND" => {
                if !args.is_empty() {
                    self.gen_expr(&args[0]);
                }
                self.emit("    call _rt_rnd");
            }
            "LEN" => {
                self.gen_expr(&args[0]);
                // String length is in rdx after gen_expr
                self.emit("    cvtsi2sd xmm0, rdx");
            }
            "LEFT$" => {
                self.gen_expr(&args[0]); // string: rax=ptr, rdx=len
                self.emit("    mov rdi, rax");
                self.emit("    mov rsi, rdx");
                self.gen_expr(&args[1]); // count
                self.emit("    cvttsd2si rdx, xmm0");
                self.emit("    call _rt_left");
            }
            "RIGHT$" => {
                self.gen_expr(&args[0]);
                self.emit("    mov rdi, rax");
                self.emit("    mov rsi, rdx");
                self.gen_expr(&args[1]);
                self.emit("    cvttsd2si rdx, xmm0");
                self.emit("    call _rt_right");
            }
            "MID$" => {
                self.gen_expr(&args[0]);
                self.emit("    mov rdi, rax");
                self.emit("    mov rsi, rdx");
                self.gen_expr(&args[1]);
                self.emit("    cvttsd2si rdx, xmm0");
                if args.len() > 2 {
                    self.gen_expr(&args[2]);
                    self.emit("    cvttsd2si rcx, xmm0");
                } else {
                    self.emit("    mov rcx, -1"); // rest of string
                }
                self.emit("    call _rt_mid");
            }
            "INSTR" => {
                // INSTR([start,] haystack$, needle$)
                let (start_arg, hay_arg, needle_arg) = if args.len() == 3 {
                    (Some(&args[0]), &args[1], &args[2])
                } else {
                    (None, &args[0], &args[1])
                };
                if let Some(start) = start_arg {
                    self.gen_expr(start);
                    self.emit("    cvttsd2si r8, xmm0");
                } else {
                    self.emit("    mov r8, 1");
                }
                self.gen_expr(hay_arg);
                self.emit("    mov rdi, rax");
                self.emit("    mov rsi, rdx");
                self.gen_expr(needle_arg);
                self.emit("    mov rdx, rax");
                self.emit("    mov rcx, rdx");
                self.emit("    call _rt_instr");
                self.emit("    cvtsi2sd xmm0, rax");
            }
            "ASC" => {
                self.gen_expr(&args[0]);
                self.emit("    movzx eax, BYTE PTR [rax]");
                self.emit("    cvtsi2sd xmm0, eax");
            }
            "CHR$" => {
                self.gen_expr(&args[0]);
                self.emit("    cvttsd2si rdi, xmm0");
                self.emit("    call _rt_chr");
            }
            "VAL" => {
                self.gen_expr(&args[0]);
                self.emit("    mov rdi, rax");
                self.emit("    mov rsi, rdx");
                self.emit("    call _rt_val");
            }
            "STR$" => {
                self.gen_expr(&args[0]);
                self.emit("    call _rt_str");
            }
            "CINT" | "CLNG" => {
                self.gen_expr(&args[0]);
                self.emit("    cvttsd2si rax, xmm0");
                self.emit("    cvtsi2sd xmm0, rax");
            }
            "CSNG" | "CDBL" => {
                self.gen_expr(&args[0]);
                // Already a double
            }
            "TIMER" => {
                self.emit("    call _rt_timer");
            }
            _ => {
                // User-defined function or array access
                if self.arrays.contains_key(&upper_name) || upper_name.ends_with('$') {
                    // Array access
                    self.gen_array_load(&upper_name, args);
                } else {
                    // User function call
                    self.gen_call(name, args);
                }
            }
        }
    }

    fn gen_call(&mut self, name: &str, args: &[Expr]) {
        // Push args in registers (System V ABI)
        let int_regs = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"];

        // Save current xmm0 if we'll use it for args
        if !args.is_empty() {
            self.emit("    sub rsp, 8");
            self.emit("    movsd QWORD PTR [rsp], xmm0");
        }

        // For simplicity, pass all numeric args as floats in integer registers
        for (i, arg) in args.iter().enumerate() {
            self.gen_expr(arg);
            if i < int_regs.len() {
                self.emit(&format!("    movq {}, xmm0", int_regs[i]));
            }
        }

        self.emit(&format!("    call _proc_{}", name));

        if !args.is_empty() {
            self.emit("    add rsp, 8");
        }
    }

    fn gen_dim_array(&mut self, arr: &ArrayDecl) {
        let elem_size = if is_string_var(&arr.name) { 16 } else { 8 };

        // First, evaluate and store all dimension bounds
        // BASIC DIM A(N) means indices 0..N (N+1 elements), so add 1 to each bound
        let mut dim_offsets = Vec::new();
        for dim in arr.dimensions.iter() {
            self.gen_expr(dim);
            self.emit("    cvttsd2si rax, xmm0");
            self.emit("    inc rax"); // DIM A(N) has N+1 elements (0 to N)
            self.stack_offset -= 8;
            dim_offsets.push(self.stack_offset);
            self.emit(&format!(
                "    mov QWORD PTR [rbp + {}], rax",
                self.stack_offset
            ));
        }

        // Calculate total elements: dim0 * dim1 * dim2 * ...
        self.emit(&format!(
            "    mov rax, QWORD PTR [rbp + {}]",
            dim_offsets[0]
        ));
        for offset in dim_offsets.iter().skip(1) {
            self.emit(&format!("    imul rax, QWORD PTR [rbp + {}]", offset));
        }

        // Allocate: total_elements * elem_size
        self.emit(&format!("    imul rdi, rax, {}", elem_size));
        self.emit(&format!("    call {}malloc", self.prefix));

        // Store array pointer
        self.stack_offset -= 8;
        let ptr_offset = self.stack_offset;
        self.emit(&format!("    mov QWORD PTR [rbp + {}], rax", ptr_offset));

        // Record array info
        self.arrays.insert(
            arr.name.clone(),
            ArrayInfo {
                ptr_offset,
                dim_offsets,
            },
        );
    }

    fn gen_array_load(&mut self, name: &str, indices: &[Expr]) {
        let arr_info = self.arrays.get(name).expect("Array not declared");
        let ptr_offset = arr_info.ptr_offset;
        let dim_offsets = arr_info.dim_offsets.clone();
        let elem_size = if is_string_var(name) { 16 } else { 8 };

        // Calculate linear index using row-major order:
        // For A(i, j, k): linear = ((i * dim1) + j) * dim2 + k
        // Start with first index
        self.gen_expr(&indices[0]);
        self.emit("    cvttsd2si rax, xmm0"); // rax = indices[0]

        // For each subsequent index, multiply by dimension bound and add
        for (i, idx_expr) in indices.iter().enumerate().skip(1) {
            // Save current accumulated index
            self.emit("    push rax");
            // Evaluate next index
            self.gen_expr(idx_expr);
            self.emit("    cvttsd2si rcx, xmm0"); // rcx = indices[i]
            self.emit("    pop rax");
            // rax = rax * dim[i] + indices[i]
            self.emit(&format!(
                "    imul rax, QWORD PTR [rbp + {}]",
                dim_offsets[i]
            ));
            self.emit("    add rax, rcx");
        }

        // Multiply by element size and add to base pointer
        self.emit(&format!("    imul rax, {}", elem_size));
        self.emit(&format!("    add rax, QWORD PTR [rbp + {}]", ptr_offset));

        // Load value from computed address
        if is_string_var(name) {
            self.emit("    mov rcx, rax");
            self.emit("    mov rax, QWORD PTR [rcx]");
            self.emit("    mov rdx, QWORD PTR [rcx + 8]");
        } else {
            self.emit("    movsd xmm0, QWORD PTR [rax]");
        }
    }

    fn gen_array_store(&mut self, name: &str, indices: &[Expr], value: &Expr) {
        let arr_info = self.arrays.get(name).expect("Array not declared");
        let ptr_offset = arr_info.ptr_offset;
        let dim_offsets = arr_info.dim_offsets.clone();
        let elem_size = if is_string_var(name) { 16 } else { 8 };

        // Calculate linear index using row-major order (same as gen_array_load)
        self.gen_expr(&indices[0]);
        self.emit("    cvttsd2si rax, xmm0");

        for (i, idx_expr) in indices.iter().enumerate().skip(1) {
            self.emit("    push rax");
            self.gen_expr(idx_expr);
            self.emit("    cvttsd2si rcx, xmm0");
            self.emit("    pop rax");
            self.emit(&format!(
                "    imul rax, QWORD PTR [rbp + {}]",
                dim_offsets[i]
            ));
            self.emit("    add rax, rcx");
        }

        // Compute final address and save it
        self.emit(&format!("    imul rax, {}", elem_size));
        self.emit(&format!("    add rax, QWORD PTR [rbp + {}]", ptr_offset));
        self.emit("    push rax"); // save address

        // Evaluate value
        self.gen_expr(value);

        // Store value at computed address
        self.emit("    pop rcx");
        if is_string_var(name) {
            self.emit("    mov QWORD PTR [rcx], rax");
            self.emit("    mov QWORD PTR [rcx + 8], rdx");
        } else {
            self.emit("    movsd QWORD PTR [rcx], xmm0");
        }
    }

    fn gen_string_assign(&mut self, name: &str, value: &Expr) {
        self.gen_expr(value);
        let offset = self.get_var_offset(name);
        // For strings, also allocate space for length
        self.stack_offset -= 8; // extra space for length
        self.emit(&format!("    mov QWORD PTR [rbp + {}], rax", offset));
        self.emit(&format!("    mov QWORD PTR [rbp + {}], rdx", offset - 8));
    }

    fn emit_data_section(&mut self) {
        self.output.push_str("\n.data\n");

        // String literals - clone to avoid borrow issues
        let strings = self.string_literals.clone();
        for (i, s) in strings.iter().enumerate() {
            self.output.push_str(&format!("_str_{}:\n", i));
            let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
            self.output
                .push_str(&format!("    .ascii \"{}\"\n", escaped));
        }

        // DATA table - always define it (even if empty) to avoid linker errors
        self.output.push_str("_data_table:\n");
        let data_items = self.data_items.clone();
        for item in &data_items {
            match item {
                Literal::Integer(n) => {
                    self.output.push_str("    .quad 0  # type int\n");
                    self.output.push_str(&format!("    .quad {}\n", n));
                }
                Literal::Float(f) => {
                    self.output.push_str("    .quad 1  # type float\n");
                    self.output
                        .push_str(&format!("    .quad 0x{:X}\n", f.to_bits()));
                }
                Literal::String(s) => {
                    let idx = self.string_literals.len();
                    self.string_literals.push(s.clone());
                    self.output.push_str("    .quad 2  # type string\n");
                    self.output.push_str(&format!("    .quad _str_{}\n", idx));
                }
            }
        }
        self.output
            .push_str(&format!("_data_count: .quad {}\n", data_items.len()));

        // DATA pointer
        self.emit("_data_ptr: .quad 0");

        // GOSUB return stack pointer
        if self.gosub_used {
            self.emit("_gosub_sp: .quad 0");
        }

        self.emit("");
        self.emit(".bss");
        // GOSUB stack (if needed)
        if self.gosub_used {
            self.emit("_gosub_stack: .skip 8192  # GOSUB return stack");
        }
    }
}

impl Default for CodeGen {
    fn default() -> Self {
        Self::new()
    }
}
