//! Compiler code for modifiers

use super::*;

impl Compiler {
    #[allow(clippy::collapsible_match)]
    pub(super) fn modified(&mut self, modified: Modified, call: bool) -> UiuaResult {
        let op_count = modified.code_operands().count();

        // De-sugar function pack
        if op_count == 1 {
            let operand = modified.code_operands().next().unwrap().clone();
            if let Sp {
                value: Word::Switch(sw @ Switch { angled: false, .. }),
                span,
            } = operand
            {
                match &modified.modifier.value {
                    Modifier::Primitive(Primitive::Dip) => {
                        let mut branches = sw.branches.into_iter().rev();
                        let mut new = Modified {
                            modifier: modified.modifier.clone(),
                            operands: vec![branches.next().unwrap().map(Word::Func)],
                        };
                        for branch in branches {
                            let mut lines = branch.value.lines;
                            (lines.last_mut().unwrap())
                                .push(span.clone().sp(Word::Modified(Box::new(new))));
                            new = Modified {
                                modifier: modified.modifier.clone(),
                                operands: vec![branch.span.clone().sp(Word::Func(Func {
                                    id: FunctionId::Anonymous(branch.span.clone()),
                                    signature: None,
                                    lines,
                                    closed: true,
                                }))],
                            };
                        }
                        return self.modified(new, call);
                    }
                    Modifier::Primitive(Primitive::Fork | Primitive::Bracket) => {
                        let mut branches = sw.branches.into_iter().rev();
                        let mut new = Modified {
                            modifier: modified.modifier.clone(),
                            operands: {
                                let mut ops: Vec<_> = branches
                                    .by_ref()
                                    .take(2)
                                    .map(|w| w.map(Word::Func))
                                    .collect();
                                ops.reverse();
                                ops
                            },
                        };
                        for branch in branches {
                            new = Modified {
                                modifier: modified.modifier.clone(),
                                operands: vec![
                                    branch.map(Word::Func),
                                    span.clone().sp(Word::Modified(Box::new(new))),
                                ],
                            };
                        }
                        return self.modified(new, call);
                    }
                    Modifier::Primitive(Primitive::Cascade) => {
                        let mut branches = sw.branches.into_iter().rev();
                        let mut new = Modified {
                            modifier: modified.modifier.clone(),
                            operands: {
                                let mut ops: Vec<_> = branches
                                    .by_ref()
                                    .take(2)
                                    .map(|w| w.map(Word::Func))
                                    .collect();
                                ops.reverse();
                                ops
                            },
                        };
                        for branch in branches {
                            new = Modified {
                                modifier: modified.modifier.clone(),
                                operands: vec![
                                    branch.map(Word::Func),
                                    span.clone().sp(Word::Modified(Box::new(new))),
                                ],
                            };
                        }
                        return self.modified(new, call);
                    }
                    modifier if modifier.args() >= 2 => {
                        if sw.branches.len() != modifier.args() {
                            return Err(self.fatal_error(
                                modified.modifier.span.clone().merge(span),
                                format!(
                                    "{} requires {} function arguments, but the \
                                    function pack has {} functions",
                                    modifier,
                                    modifier.args(),
                                    sw.branches.len()
                                ),
                            ));
                        }
                        let new = Modified {
                            modifier: modified.modifier.clone(),
                            operands: sw.branches.into_iter().map(|w| w.map(Word::Func)).collect(),
                        };
                        return self.modified(new, call);
                    }
                    modifier => 'blk: {
                        if let Modifier::Ref(name) = modifier {
                            if let Ok((_, local)) = self.ref_local(name) {
                                if self.array_macros.contains_key(&local.index) {
                                    break 'blk;
                                }
                            }
                        }
                        return Err(self.fatal_error(
                            modified.modifier.span.clone().merge(span),
                            format!(
                                "{modifier} cannot use a function pack. If you meant to \
                                use a switch function, add a layer of parentheses."
                            ),
                        ));
                    }
                }
            }
        }

        if op_count == modified.modifier.value.args() {
            // Inlining
            if self.inline_modifier(&modified, call)? {
                return Ok(());
            }
        } else {
            // Validate operand count
            return Err(self.fatal_error(
                modified.modifier.span.clone(),
                format!(
                    "{} requires {} function argument{}, but {} {} provided",
                    modified.modifier.value,
                    modified.modifier.value.args(),
                    if modified.modifier.value.args() == 1 {
                        ""
                    } else {
                        "s"
                    },
                    op_count,
                    if op_count == 1 { "was" } else { "were" }
                ),
            ));
        }

        // Handle macros
        let prim = match modified.modifier.value {
            Modifier::Primitive(prim) => prim,
            Modifier::Ref(r) => {
                let (path_locals, local) = self.ref_local(&r)?;
                self.validate_local(&r.name.value, local, &r.name.span);
                self.asm
                    .global_references
                    .insert(r.name.clone(), local.index);
                for (local, comp) in path_locals.into_iter().zip(&r.path) {
                    (self.asm.global_references).insert(comp.module.clone(), local.index);
                }
                if let Some(mut words) = self.stack_macros.get(&local.index).cloned() {
                    // Stack macros
                    if self.macro_depth > 20 {
                        return Err(self.fatal_error(
                            modified.modifier.span.clone(),
                            "Macro recursion detected",
                        ));
                    }
                    self.macro_depth += 1;
                    let instrs = self
                        .expand_macro(&mut words, modified.operands)
                        .and_then(|()| self.compile_words(words, true));
                    self.macro_depth -= 1;
                    let instrs = instrs?;
                    match instrs_signature(&instrs) {
                        Ok(sig) => {
                            let func = self.add_function(
                                FunctionId::Named(r.name.value.clone()),
                                sig,
                                instrs,
                            );
                            self.push_instr(Instr::PushFunc(func));
                        }
                        Err(e) => self.add_error(
                            modified.modifier.span.clone(),
                            format!("Cannot infer function signature: {e}"),
                        ),
                    }
                    if call {
                        let span = self.add_span(modified.modifier.span);
                        self.push_instr(Instr::Call(span));
                    }
                } else if let Some(function) = self.array_macros.get(&local.index) {
                    // Array macros

                    // Collect operands as strings
                    let mut operands: Vec<Sp<Word>> = (modified.operands.into_iter())
                        .filter(|w| w.value.is_code())
                        .collect();
                    if operands.len() == 1 {
                        let operand = operands.remove(0);
                        operands = match operand.value {
                            Word::Switch(sw) => {
                                sw.branches.into_iter().map(|b| b.map(Word::Func)).collect()
                            }
                            word => vec![operand.span.sp(word)],
                        };
                    }
                    let formatted: Array<Boxed> = operands
                        .iter()
                        .map(|w| Boxed(format_word(w, &self.asm.inputs).into()))
                        .collect();

                    // Run the macro function
                    let mut env = Uiua::with_backend(self.backend.clone());
                    let val = match env.run_asm(&self.asm).and_then(|()| {
                        env.push(formatted);
                        env.call(function.clone())?;
                        env.pop("macro result")
                    }) {
                        Ok(val) => val,
                        Err(e) => {
                            return Err(self.fatal_error(
                                modified.modifier.span.clone(),
                                format!("Macro failed: {e}"),
                            ));
                        }
                    };

                    // Parse the macro output
                    let mut code = String::new();
                    if let Ok(s) = val.as_string(&env, "") {
                        code = s;
                    } else {
                        for row in val.into_rows() {
                            let s = row.as_string(&env, "Macro output rows must be strings")?;
                            if !code.is_empty() {
                                code.push(' ');
                            }
                            code.push_str(&s);
                        }
                    }
                    self.backend = env.rt.backend;

                    // Quote
                    self.quote(&code, &modified.modifier.span, call)?;
                } else {
                    panic!("Macro not found")
                }

                return Ok(());
            }
        };

        // Give advice about redundancy
        match prim {
            m @ Primitive::Each if self.macro_depth == 0 => {
                if let [Sp {
                    value: Word::Primitive(prim),
                    span,
                }] = modified.operands.as_slice()
                {
                    if prim.class().is_pervasive() {
                        let span = modified.modifier.span.clone().merge(span.clone());
                        self.emit_diagnostic(
                            format!(
                                "Using {m} with a pervasive primitive like {p} is \
                                redundant. Just use {p} by itself.",
                                m = m.format(),
                                p = prim.format(),
                            ),
                            DiagnosticKind::Advice,
                            span,
                        );
                    }
                } else if words_look_pervasive(&modified.operands) {
                    let span = modified.modifier.span.clone();
                    self.emit_diagnostic(
                        format!(
                            "{m}'s function is pervasive, \
                                so {m} is redundant here.",
                            m = m.format()
                        ),
                        DiagnosticKind::Advice,
                        span,
                    );
                }
            }
            _ => {}
        }

        // Compile operands
        let instrs = self.compile_words(modified.operands, false)?;

        // Reduce monadic deprectation message
        if let (Modifier::Primitive(Primitive::Reduce), [Instr::PushFunc(f)]) =
            (&modified.modifier.value, instrs.as_slice())
        {
            if f.signature().args == 1 {
                self.emit_diagnostic(
                    format!(
                        "{} with a monadic function is deprecated. \
                        Prefer {} with stack array notation, i.e. `°[⊙⊙∘]`",
                        Primitive::Reduce.format(),
                        Primitive::Un.format()
                    ),
                    DiagnosticKind::Warning,
                    modified.modifier.span.clone(),
                );
            }
        }

        if call {
            self.push_all_instrs(instrs);
            self.primitive(prim, modified.modifier.span, true);
        } else {
            self.new_functions.push(EcoVec::new());
            self.push_all_instrs(instrs);
            self.primitive(prim, modified.modifier.span.clone(), true);
            let instrs = self.new_functions.pop().unwrap();
            match instrs_signature(&instrs) {
                Ok(sig) => {
                    let func = self.add_function(
                        FunctionId::Anonymous(modified.modifier.span),
                        sig,
                        instrs,
                    );
                    self.push_instr(Instr::PushFunc(func));
                }
                Err(e) => self.add_error(
                    modified.modifier.span.clone(),
                    format!("Cannot infer function signature: {e}"),
                ),
            }
        }
        Ok(())
    }
    pub(super) fn inline_modifier(&mut self, modified: &Modified, call: bool) -> UiuaResult<bool> {
        use Primitive::*;
        let Modifier::Primitive(prim) = modified.modifier.value else {
            return Ok(false);
        };
        macro_rules! finish {
            ($instrs:expr, $sig:expr) => {{
                if call {
                    self.push_all_instrs($instrs);
                } else {
                    let func = self.add_function(
                        FunctionId::Anonymous(modified.modifier.span.clone()),
                        $sig,
                        $instrs.to_vec(),
                    );
                    self.push_instr(Instr::PushFunc(func));
                }
            }};
        }
        match prim {
            Dip | Gap | On => {
                // Compile operands
                let (mut instrs, sig) = self.compile_operand_word(modified.operands[0].clone())?;
                // Dip (|1 …) . diagnostic
                if prim == Dip && sig == (1, 1) {
                    if let Some(Instr::Prim(Dup, dup_span)) =
                        self.new_functions.last().and_then(|instrs| instrs.last())
                    {
                        if let Span::Code(dup_span) = self.get_span(*dup_span) {
                            let span = modified.modifier.span.clone().merge(dup_span);
                            self.emit_diagnostic(
                                "Prefer `⟜(…)` over `⊙(…).` for clarity",
                                DiagnosticKind::Style,
                                span,
                            );
                        }
                    }
                }

                let span = self.add_span(modified.modifier.span.clone());
                let sig = match prim {
                    Dip => {
                        instrs.insert(
                            0,
                            Instr::PushTemp {
                                stack: TempStack::Inline,
                                count: 1,
                                span,
                            },
                        );
                        instrs.push(Instr::PopTemp {
                            stack: TempStack::Inline,
                            count: 1,
                            span,
                        });
                        Signature::new(sig.args + 1, sig.outputs + 1)
                    }
                    Gap => {
                        instrs.insert(0, Instr::Prim(Pop, span));
                        Signature::new(sig.args + 1, sig.outputs)
                    }
                    On => {
                        instrs.insert(
                            0,
                            Instr::CopyToTemp {
                                stack: TempStack::Inline,
                                count: 1,
                                span,
                            },
                        );
                        instrs.push(Instr::PopTemp {
                            stack: TempStack::Inline,
                            count: 1,
                            span,
                        });
                        Signature::new(sig.args, sig.outputs + 1)
                    }
                    _ => unreachable!(),
                };
                if call {
                    self.push_instr(Instr::PushSig(sig));
                    self.push_all_instrs(instrs);
                    self.push_instr(Instr::PopSig);
                } else {
                    let func = self.add_function(
                        FunctionId::Anonymous(modified.modifier.span.clone()),
                        sig,
                        instrs,
                    );
                    self.push_instr(Instr::PushFunc(func));
                }
            }
            Fork => {
                let mut operands = modified.code_operands().cloned();
                let first_op = operands.next().unwrap();
                // ⊃∘ diagnostic
                if let Word::Primitive(Primitive::Identity) = first_op.value {
                    self.emit_diagnostic(
                        "Prefer `⟜` over `⊃∘` for clarity",
                        DiagnosticKind::Style,
                        modified.modifier.span.clone().merge(first_op.span.clone()),
                    );
                }
                let (a_instrs, a_sig) = self.compile_operand_word(first_op)?;
                let (b_instrs, b_sig) = self.compile_operand_word(operands.next().unwrap())?;
                let span = self.add_span(modified.modifier.span.clone());
                let count = a_sig.args.max(b_sig.args);
                let mut instrs = vec![Instr::PushTemp {
                    stack: TempStack::Inline,
                    count,
                    span,
                }];
                if b_sig.args > 0 {
                    instrs.push(Instr::CopyFromTemp {
                        stack: TempStack::Inline,
                        offset: count - b_sig.args,
                        count: b_sig.args,
                        span,
                    });
                }
                instrs.extend(b_instrs);
                if count - a_sig.args > 0 {
                    instrs.push(Instr::DropTemp {
                        stack: TempStack::Inline,
                        count: count - a_sig.args,
                        span,
                    });
                }
                instrs.push(Instr::PopTemp {
                    stack: TempStack::Inline,
                    count: a_sig.args,
                    span,
                });
                instrs.extend(a_instrs);
                let sig = Signature::new(a_sig.args.max(b_sig.args), a_sig.outputs + b_sig.outputs);
                if call {
                    self.push_instr(Instr::PushSig(sig));
                    self.push_all_instrs(instrs);
                    self.push_instr(Instr::PopSig);
                } else {
                    let func = self.add_function(
                        FunctionId::Anonymous(modified.modifier.span.clone()),
                        sig,
                        instrs,
                    );
                    self.push_instr(Instr::PushFunc(func));
                }
            }
            Cascade => {
                let mut operands = modified.code_operands().cloned();
                let (a_instrs, a_sig) = self.compile_operand_word(operands.next().unwrap())?;
                let (b_instrs, b_sig) = self.compile_operand_word(operands.next().unwrap())?;
                let span = self.add_span(modified.modifier.span.clone());
                let count = a_sig.args.saturating_sub(b_sig.outputs);
                if a_sig.args < b_sig.outputs {
                    self.emit_diagnostic(
                        format!(
                            "{}'s second function has more outputs \
                            than its first function has arguments, \
                            so {} is redundant here.",
                            prim.format(),
                            prim.format()
                        ),
                        DiagnosticKind::Advice,
                        modified.modifier.span.clone(),
                    );
                }
                let mut instrs = Vec::new();
                if count > 0 {
                    instrs.push(Instr::CopyToTemp {
                        stack: TempStack::Inline,
                        count,
                        span,
                    });
                }
                instrs.extend(b_instrs);
                if count > 0 {
                    instrs.push(Instr::PopTemp {
                        stack: TempStack::Inline,
                        count,
                        span,
                    });
                }
                instrs.extend(a_instrs);
                let sig = Signature::new(
                    b_sig.args.max(count),
                    a_sig.outputs.max(count.saturating_sub(b_sig.outputs)),
                );
                if call {
                    self.push_instr(Instr::PushSig(sig));
                    self.push_all_instrs(instrs);
                    self.push_instr(Instr::PopSig);
                } else {
                    let func = self.add_function(
                        FunctionId::Anonymous(modified.modifier.span.clone()),
                        sig,
                        instrs,
                    );
                    self.push_instr(Instr::PushFunc(func));
                }
            }
            Bracket => {
                let mut operands = modified.code_operands().cloned();
                let (a_instrs, a_sig) = self.compile_operand_word(operands.next().unwrap())?;
                let (b_instrs, b_sig) = self.compile_operand_word(operands.next().unwrap())?;
                let span = self.add_span(modified.modifier.span.clone());
                let mut instrs = vec![Instr::PushTemp {
                    stack: TempStack::Inline,
                    count: a_sig.args,
                    span,
                }];
                instrs.extend(b_instrs);
                instrs.push(Instr::PopTemp {
                    stack: TempStack::Inline,
                    count: a_sig.args,
                    span,
                });
                instrs.extend(a_instrs);
                let sig = Signature::new(a_sig.args + b_sig.args, a_sig.outputs + b_sig.outputs);
                if call {
                    self.push_instr(Instr::PushSig(sig));
                    self.push_all_instrs(instrs);
                    self.push_instr(Instr::PopSig);
                } else {
                    let func = self.add_function(
                        FunctionId::Anonymous(modified.modifier.span.clone()),
                        sig,
                        instrs,
                    );
                    self.push_instr(Instr::PushFunc(func));
                }
            }
            Un => {
                let mut operands = modified.code_operands().cloned();
                let f = operands.next().unwrap();
                let span = f.span.clone();
                let (instrs, _) = self.compile_operand_word(f)?;
                if let Some(inverted) = invert_instrs(&instrs, self) {
                    let sig = instrs_signature(&inverted).map_err(|e| {
                        self.fatal_error(
                            span.clone(),
                            format!("Cannot infer function signature: {e}"),
                        )
                    })?;
                    if call {
                        self.push_all_instrs(inverted);
                    } else {
                        let id = FunctionId::Anonymous(modified.modifier.span.clone());
                        let func = self.add_function(id, sig, inverted);
                        self.push_instr(Instr::PushFunc(func));
                    }
                } else {
                    return Err(self.fatal_error(span, "No inverse found"));
                }
            }
            Under => {
                let mut operands = modified.code_operands().cloned();
                let f = operands.next().unwrap();
                let f_span = f.span.clone();
                let (f_instrs, _) = self.compile_operand_word(f)?;
                let (g_instrs, g_sig) = self.compile_operand_word(operands.next().unwrap())?;
                if let Some((f_before, f_after)) = under_instrs(&f_instrs, g_sig, self) {
                    let before_sig = instrs_signature(&f_before).map_err(|e| {
                        self.fatal_error(
                            f_span.clone(),
                            format!("Cannot infer function signature: {e}"),
                        )
                    })?;
                    let after_sig = instrs_signature(&f_after).map_err(|e| {
                        self.fatal_error(
                            f_span.clone(),
                            format!("Cannot infer function signature: {e}"),
                        )
                    })?;
                    let mut instrs = if call {
                        eco_vec![Instr::PushSig(before_sig)]
                    } else {
                        EcoVec::new()
                    };
                    instrs.extend(f_before);
                    if call {
                        instrs.push(Instr::PopSig);
                    }
                    instrs.extend(g_instrs);
                    if call {
                        instrs.push(Instr::PushSig(after_sig));
                    }
                    instrs.extend(f_after);
                    if call {
                        instrs.push(Instr::PopSig);
                    }
                    if call {
                        self.push_all_instrs(instrs);
                    } else {
                        match instrs_signature(&instrs) {
                            Ok(sig) => {
                                let func = self.add_function(
                                    FunctionId::Anonymous(modified.modifier.span.clone()),
                                    sig,
                                    instrs,
                                );
                                self.push_instr(Instr::PushFunc(func));
                            }
                            Err(e) => self.add_error(
                                modified.modifier.span.clone(),
                                format!("Cannot infer function signature: {e}"),
                            ),
                        }
                    }
                } else {
                    return Err(self.fatal_error(f_span, "No inverse found"));
                }
            }
            Both => {
                let mut operands = modified.code_operands().cloned();
                let (mut instrs, sig) = self.compile_operand_word(operands.next().unwrap())?;
                let span = self.add_span(modified.modifier.span.clone());
                instrs.insert(
                    0,
                    Instr::PushTemp {
                        stack: TempStack::Inline,
                        count: sig.args,
                        span,
                    },
                );
                instrs.push(Instr::PopTemp {
                    stack: TempStack::Inline,
                    count: sig.args,
                    span,
                });
                for i in 1..instrs.len() - 1 {
                    instrs.push(instrs[i].clone());
                }
                let sig = Signature::new(sig.args * 2, sig.outputs * 2);
                if call {
                    self.push_instr(Instr::PushSig(sig));
                    self.push_all_instrs(instrs);
                    self.push_instr(Instr::PopSig);
                } else {
                    let func = self.add_function(
                        FunctionId::Anonymous(modified.modifier.span.clone()),
                        sig,
                        instrs,
                    );
                    self.push_instr(Instr::PushFunc(func));
                }
            }
            Bind => {
                let operand = modified.code_operands().next().cloned().unwrap();
                let operand_span = operand.span.clone();
                self.scope.bind_locals.push(HashSet::new());
                let (mut instrs, mut sig) = self.compile_operand_word(operand)?;
                let locals = self.scope.bind_locals.pop().unwrap();
                let local_count = locals.into_iter().max().map_or(0, |i| i + 1);
                let span = self.add_span(modified.modifier.span.clone());
                sig.args += local_count;
                if sig.args < 3 {
                    self.emit_diagnostic(
                        format!(
                            "{} should be reserved for functions with at least 3 arguments, \
                            but this function has {} arguments",
                            Bind.format(),
                            sig.args
                        ),
                        DiagnosticKind::Advice,
                        operand_span,
                    );
                }
                instrs.insert(
                    0,
                    Instr::PushLocals {
                        count: sig.args,
                        span,
                    },
                );
                instrs.push(Instr::PopLocals);
                finish!(instrs, sig);
            }
            Comptime => {
                let word = modified.code_operands().next().unwrap().clone();
                self.do_comptime(prim, word, &modified.modifier.span, call)?;
            }
            Reduce => {
                // Reduce content
                let operand = modified.code_operands().next().cloned().unwrap();
                let Word::Modified(m) = &operand.value else {
                    return Ok(false);
                };
                let Modifier::Primitive(Content) = &m.modifier.value else {
                    return Ok(false);
                };
                if m.code_operands().count() != 1 {
                    return Ok(false);
                }
                let operand = m.code_operands().next().cloned().unwrap();
                let (content_instrs, sig) = self.compile_operand_word(operand)?;
                if sig.args == 1 {
                    self.emit_diagnostic(
                        format!(
                            "{} with a monadic function is deprecated. \
                                        Prefer {} with stack array notation, i.e. `°[⊙⊙∘]`",
                            Primitive::Reduce.format(),
                            Primitive::Un.format()
                        ),
                        DiagnosticKind::Warning,
                        modified.modifier.span.clone(),
                    );
                }
                let content_func = self.add_function(
                    FunctionId::Anonymous(m.modifier.span.clone()),
                    sig,
                    content_instrs,
                );
                let span = self.add_span(modified.modifier.span.clone());
                let instrs = eco_vec![
                    Instr::PushFunc(content_func),
                    Instr::ImplPrim(ImplPrimitive::ReduceContent, span),
                ];
                finish!(instrs, Signature::new(1, 1));
            }
            Content => {
                let operand = modified.code_operands().next().cloned().unwrap();
                let (instrs, sig) = self.compile_operand_word(operand)?;
                let mut prefix = EcoVec::new();
                let span = self.add_span(modified.modifier.span.clone());
                if sig.args > 0 {
                    if sig.args > 1 {
                        prefix.push(Instr::PushTemp {
                            stack: TempStack::Inline,
                            count: sig.args - 1,
                            span,
                        });
                        for _ in 0..sig.args - 1 {
                            prefix.extend([
                                Instr::ImplPrim(ImplPrimitive::InvBox, span),
                                Instr::PopTemp {
                                    stack: TempStack::Inline,
                                    count: 1,
                                    span,
                                },
                            ]);
                        }
                    }
                    prefix.push(Instr::ImplPrim(ImplPrimitive::InvBox, span));
                }
                prefix.extend(instrs);
                finish!(prefix, sig);
            }
            Stringify => {
                let operand = modified.code_operands().next().unwrap();
                let s = format_word(operand, &self.asm.inputs);
                let instr = Instr::Push(s.into());
                finish!([instr], Signature::new(0, 1));
            }
            Quote => {
                let operand = modified.code_operands().next().unwrap().clone();
                self.new_functions.push(EcoVec::new());
                self.do_comptime(prim, operand, &modified.modifier.span, true)?;
                let instrs = self.new_functions.pop().unwrap();
                let code: String = match instrs.as_slice() {
                    [Instr::Push(Value::Char(chars))] if chars.rank() == 1 => {
                        chars.data.iter().collect()
                    }
                    [Instr::Push(Value::Char(chars))] => {
                        return Err(self.fatal_error(
                            modified.modifier.span.clone(),
                            format!(
                                "quote's argument compiled to a \
                                rank {} array rather than a string",
                                chars.rank()
                            ),
                        ))
                    }
                    [Instr::Push(value)] => {
                        return Err(self.fatal_error(
                            modified.modifier.span.clone(),
                            format!(
                                "quote's argument compiled to a \
                                {} array rather than a string",
                                value.type_name()
                            ),
                        ))
                    }
                    _ => {
                        return Err(self.fatal_error(
                            modified.modifier.span.clone(),
                            "quote's argument did not compile to a string",
                        ));
                    }
                };
                self.quote(&code, &modified.modifier.span, call)?;
            }
            Sig => {
                let operand = modified.code_operands().next().unwrap().clone();
                let (_, sig) = self.compile_operand_word(operand)?;
                let instrs = [
                    Instr::Push(sig.outputs.into()),
                    Instr::Push(sig.args.into()),
                ];
                finish!(instrs, Signature::new(0, 2));
            }
            _ => return Ok(false),
        }
        self.handle_primitive_experimental(prim, &modified.modifier.span);
        self.handle_primitive_deprecation(prim, &modified.modifier.span);
        Ok(true)
    }
    fn quote(&mut self, code: &str, span: &CodeSpan, call: bool) -> UiuaResult {
        let (items, errors, _) = parse(
            code,
            InputSrc::Macro(span.clone().into()),
            &mut self.asm.inputs,
        );
        if let Some(error) = errors.first() {
            return Err(self.fatal_error(span.clone(), format!("Macro failed: {error}")));
        }

        // Extract the words
        if items.len() != 1 {
            return Err(self.fatal_error(
                span.clone(),
                format!(
                    "Macro must generate 1 item, but it generated {}",
                    items.len()
                ),
            ));
        }
        let item = items.into_iter().next().unwrap();
        let words = match item {
            Item::Words(words) => words,
            Item::Binding(_) => {
                return Err(self.fatal_error(
                    span.clone(),
                    "Macro must generate words, but it generated a binding",
                ));
            }
            Item::Import(_) => {
                return Err(self.fatal_error(
                    span.clone(),
                    "Macro must generate words, but it generated an import",
                ));
            }
            Item::TestScope(_) => {
                return Err(self.fatal_error(
                    span.clone(),
                    "Macro must generate words, but it generated a test scope",
                ));
            }
        };

        // Compile the generated words
        for line in words {
            self.words(line, call)?;
        }
        Ok(())
    }
    fn do_comptime(
        &mut self,
        prim: Primitive,
        operand: Sp<Word>,
        span: &CodeSpan,
        call: bool,
    ) -> UiuaResult {
        let mut comp = self.clone();
        let (instrs, sig) = comp.compile_operand_word(operand)?;
        if sig.args > 0 {
            return Err(self.fatal_error(
                span.clone(),
                format!(
                    "{}'s function must have no arguments, but it has {}",
                    prim.format(),
                    sig.args
                ),
            ));
        }
        let instrs = optimize_instrs(instrs, true, &comp);
        let start = comp.asm.instrs.len();
        let len = instrs.len();
        comp.asm.instrs.extend(instrs);
        comp.asm.top_slices.push(FuncSlice { start, len });
        let mut env = Uiua::with_backend(self.backend.clone());
        let values = match env.run_asm(&comp.asm) {
            Ok(_) => env.take_stack(),
            Err(e) => {
                if self.errors.is_empty() {
                    self.add_error(span.clone(), format!("Compile-time evaluation failed: {e}"));
                }
                vec![Value::default(); sig.outputs]
            }
        };
        self.backend = env.rt.backend;
        if !call {
            self.new_functions.push(EcoVec::new());
        }
        let val_count = sig.outputs;
        for value in values.into_iter().rev().take(val_count).rev() {
            self.push_instr(Instr::push(value));
        }
        if !call {
            let instrs = self.new_functions.pop().unwrap();
            let sig = Signature::new(0, val_count);
            let func = self.add_function(FunctionId::Anonymous(span.clone()), sig, instrs);
            self.push_instr(Instr::PushFunc(func));
        }
        Ok(())
    }
}
