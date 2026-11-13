//! Scoped/conditional aggregates write directly into their caller-owned destination.
use super::{end_line_of, err, init_aggregate_at, line_of, stmt, FnLower, Ty};
use crate::VReg;
use std::collections::HashMap;
use syn::{Block, Expr, Stmt};

fn block_value(l: &mut FnLower, dst: VReg, ty: &Ty, block: &Block) -> Result<(), syn::Error> {
    l.scopes.push(HashMap::new());
    l.scope_ends.push(end_line_of(block));
    let (head, tail) = match block.stmts.split_last() {
        Some((Stmt::Expr(tail), head)) => (head, Some(tail)),
        _ => (block.stmts.as_slice(), None),
    };
    for statement in head {
        if l.dead {
            return Err(err(statement, "unreachable aggregate branch statement"));
        }
        stmt(l, statement)?;
    }
    if !l.dead {
        let tail = tail.ok_or_else(|| err(block, "aggregate block needs a tail value"))?;
        l.b.set_line_hint(line_of(tail));
        init_aggregate_at(l, dst, ty, tail)?;
    }
    l.scope_ends.pop();
    l.scopes.pop();
    Ok(())
}

pub(super) fn initialize(
    l: &mut FnLower,
    dst: VReg,
    ty: &Ty,
    value: &Expr,
) -> Result<bool, syn::Error> {
    match value {
        Expr::Paren(p) => init_aggregate_at(l, dst, ty, &p.expr)?,
        Expr::Block(b) => block_value(l, dst, ty, &b.block)?,
        Expr::If(branch) => {
            let (_, otherwise) = branch
                .else_branch
                .as_ref()
                .ok_or_else(|| err(branch, "aggregate if-expression needs else"))?;
            let yes = l.b.raw_block(&[]);
            let no = l.b.raw_block(&[]);
            let join = l.b.raw_block(&[]);
            super::cond_lazy(l, &branch.cond, yes, no)?;
            l.b.enter_block(yes);
            block_value(l, dst, ty, &branch.then_branch)?;
            let yes_dead = l.dead;
            l.dead = false;
            l.b.mid_if_else(no, join);
            init_aggregate_at(l, dst, ty, otherwise)?;
            let no_dead = l.dead;
            l.b.end_if_else(join);
            l.dead = yes_dead && no_dead;
        }
        _ => return Ok(false),
    }
    Ok(true)
}
