use std::{collections::HashMap, ops::Neg as _};

use ark_ff::{One as _, Zero};

use crate::{
    circuit_writer::{CircuitWriter, GateKind, VarInfo},
    constants::{Field, Span},
    error::{Error, ErrorKind, Result},
    imports::{FnHandle, FnKind},
    lexer::Token,
    parser::{FnSig, ParserCtx, TyKind},
    type_checker::FnInfo,
    var::{ConstOrCell, Var},
};

use self::crypto::CRYPTO_FNS;

pub mod crypto;

#[derive(Clone)]
pub struct ImportedModule {
    pub name: String,
    pub functions: HashMap<String, FnInfo>,
    // TODO: delete?
    pub span: Span,
}

impl std::fmt::Debug for ImportedModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ImportedModule {{ name: {:?}, functions: {:?}, span: {:?} }}",
            self.name,
            self.functions.keys(),
            self.span
        )
    }
}

/// Parses the rest of a `use std::` statement. Returns a list of functions to import in the scope.
pub fn parse_std_import(submodule: &str, span: Span) -> Result<ImportedModule> {
    let mut res = ImportedModule {
        name: submodule.to_string(),
        functions: HashMap::new(),
        span,
    };

    // TODO: make sure we're not importing the same module twice no?
    match submodule {
        "crypto" => {
            let crypto_functions = parse_fn_sigs(&CRYPTO_FNS);
            for func in crypto_functions {
                res.functions
                    .insert(func.sig().name.name.value.clone(), func);
            }
        }
        _ => return Err(Error::new(ErrorKind::StdImport("unknown module"), span)),
    }

    Ok(res)
}

/// Takes a list of function signatures (as strings) and their associated function pointer,
/// returns the same list but with the parsed functions (as [FunctionSig]).
pub fn parse_fn_sigs(fn_sigs: &[(&str, FnHandle)]) -> Vec<FnInfo> {
    let mut functions: Vec<FnInfo> = vec![];
    let ctx = &mut ParserCtx::default();

    for (sig, fn_ptr) in fn_sigs {
        let mut tokens = Token::parse(sig).unwrap();

        let sig = FnSig::parse(ctx, &mut tokens).unwrap();

        functions.push(FnInfo {
            kind: FnKind::BuiltIn(sig, *fn_ptr),
            span: Span::default(),
        });
    }

    functions
}

//
// Builtins or utils (imported by default)
// TODO: give a name that's useful for the user,
//       not something descriptive internally like "builtins"

const ASSERT_FN: &str = "assert(condition: Bool)";
const ASSERT_EQ_FN: &str = "assert_eq(lhs: Field, rhs: Field)";

pub const BUILTIN_FNS: [(&str, FnHandle); 2] = [(ASSERT_EQ_FN, assert_eq), (ASSERT_FN, assert)];

/// Asserts that two vars are equal.
fn assert_eq(compiler: &mut CircuitWriter, vars: &[VarInfo], span: Span) -> Result<Option<Var>> {
    // we get two vars
    assert_eq!(vars.len(), 2);
    let lhs_info = &vars[0];
    let rhs_info = &vars[1];

    // they are both of type field
    if !matches!(lhs_info.typ, Some(TyKind::Field | TyKind::BigInt)) {
        panic!(
            "the lhs of assert_eq must be of type Field or BigInt. It was of type {:?}",
            lhs_info.typ
        );
    }

    if !matches!(rhs_info.typ, Some(TyKind::Field | TyKind::BigInt)) {
        panic!(
            "the rhs of assert_eq must be of type Field or BigInt. It was of type {:?}",
            rhs_info.typ
        );
    }

    // retrieve the values
    let lhs_var = &lhs_info.var;
    assert_eq!(lhs_var.len(), 1);
    let lhs_cvar = &lhs_var[0];

    let rhs_var = &rhs_info.var;
    assert_eq!(rhs_var.len(), 1);
    let rhs_cvar = &rhs_var[0];

    match (lhs_cvar, rhs_cvar) {
        // two constants
        (ConstOrCell::Const(a), ConstOrCell::Const(b)) => {
            if a != b {
                return Err(Error::new(ErrorKind::AssertionFailed, span));
            }
        }

        // a const and a var
        (ConstOrCell::Const(cst), ConstOrCell::Cell(cvar))
        | (ConstOrCell::Cell(cvar), ConstOrCell::Const(cst)) => {
            compiler.add_gate(
                "constrain var - cst = 0 to check equality",
                GateKind::DoubleGeneric,
                vec![Some(*cvar)],
                vec![
                    Field::one(),
                    Field::zero(),
                    Field::zero(),
                    Field::zero(),
                    cst.neg(),
                ],
                span,
            );
        }
        (ConstOrCell::Cell(lhs), ConstOrCell::Cell(rhs)) => {
            // TODO: use permutation to check that
            compiler.add_gate(
                "constrain lhs - rhs = 0 to assert that they are equal",
                GateKind::DoubleGeneric,
                vec![Some(*lhs), Some(*rhs)],
                vec![Field::one(), Field::one().neg()],
                span,
            );
        }
    }

    Ok(None)
}

/// Asserts that a condition is true.
fn assert(compiler: &mut CircuitWriter, vars: &[VarInfo], span: Span) -> Result<Option<Var>> {
    // we get a single var
    assert_eq!(vars.len(), 1);

    // of type bool
    let var_info = &vars[0];
    assert!(matches!(var_info.typ, Some(TyKind::Bool)));

    // of only one field element
    let var = &var_info.var;
    assert_eq!(var.len(), 1);
    let cond = &var[0];

    match cond {
        ConstOrCell::Const(cst) => {
            assert!(cst.is_one());
        }
        ConstOrCell::Cell(cvar) => {
            // TODO: use permutation to check that
            let zero = Field::zero();
            let one = Field::one();
            compiler.add_gate(
                "constrain 1 - X = 0 to assert that X is true",
                GateKind::DoubleGeneric,
                vec![None, Some(*cvar)],
                // use the constant to constrain 1 - X = 0
                vec![zero, one.neg(), zero, zero, one],
                span,
            );
        }
    }

    Ok(None)
}
