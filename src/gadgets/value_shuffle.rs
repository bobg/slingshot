use super::scalar_shuffle;
use bulletproofs::r1cs::{ConstraintSystem, RandomizedConstraintSystem};
use error::SpacesuitError;
use value::AllocatedValue;

/// Enforces that the output values `y` are a valid reordering of the inputs values `x`.
/// The inputs and outputs are all of the `AllocatedValue` type, which contains the fields
/// quantity, issuer, and tag. Works for `k` inputs and `k` outputs.
pub fn fill_cs<CS: ConstraintSystem>(
    cs: &mut CS,
    x: Vec<AllocatedValue>,
    y: Vec<AllocatedValue>,
) -> Result<(), SpacesuitError> {
    if x.len() != y.len() {
        return Err(SpacesuitError::InvalidR1CSConstruction);
    }
    let k = x.len();
    if k == 1 {
        let x = x[0];
        let y = y[0];
        cs.constrain(y.q - x.q);
        cs.constrain(y.a - x.a);
        cs.constrain(y.t - x.t);
        return Ok(());
    }

    cs.specify_randomized_constraints(move |cs| {
        let w = cs.challenge_scalar(b"k-value shuffle challenge");
        let w2 = w * w;
        let mut x_scalars = Vec::with_capacity(k);
        let mut y_scalars = Vec::with_capacity(k);

        for i in 0..k {
            let (x_i_var, y_i_var, _) = cs.multiply(
                x[i].q + x[i].a * w + x[i].t * w2,
                y[i].q + y[i].a * w + y[i].t * w2,
            );
            x_scalars.push(x_i_var);
            y_scalars.push(y_i_var);
        }

        scalar_shuffle::fill_cs(cs, x_scalars, y_scalars);

        Ok(())
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bulletproofs::r1cs::{Prover, Verifier};
    use bulletproofs::{BulletproofGens, PedersenGens};
    use merlin::Transcript;
    use value::{ProverCommittable, Value, VerifierCommittable};

    // Helper functions to make the tests easier to read
    fn yuan(q: u64) -> Value {
        Value {
            q,
            a: 888u64.into(),
            t: 999u64.into(),
        }
    }
    fn peso(q: u64) -> Value {
        Value {
            q,
            a: 666u64.into(),
            t: 777u64.into(),
        }
    }
    fn euro(q: u64) -> Value {
        Value {
            q,
            a: 444u64.into(),
            t: 555u64.into(),
        }
    }
    fn wrong() -> Value {
        Value {
            q: 9991u64,
            a: 9992u64.into(),
            t: 9993u64.into(),
        }
    }

    #[test]
    fn value_shuffle() {
        // k=1
        assert!(value_shuffle_helper(vec![peso(1)], vec![peso(1)]).is_ok());
        assert!(value_shuffle_helper(vec![yuan(4)], vec![yuan(4)]).is_ok());
        assert!(value_shuffle_helper(vec![peso(1)], vec![yuan(4)]).is_err());
        // k=2
        assert!(value_shuffle_helper(vec![peso(1), yuan(4)], vec![peso(1), yuan(4)]).is_ok());
        assert!(value_shuffle_helper(vec![peso(1), yuan(4)], vec![yuan(4), peso(1)]).is_ok());
        assert!(value_shuffle_helper(vec![yuan(4), yuan(4)], vec![yuan(4), yuan(4)]).is_ok());
        assert!(value_shuffle_helper(vec![peso(1), peso(1)], vec![yuan(4), peso(1)]).is_err());
        assert!(value_shuffle_helper(vec![peso(1), yuan(4)], vec![peso(1), yuan(4)]).is_ok());
        // k=3
        assert!(
            value_shuffle_helper(
                vec![peso(1), yuan(4), euro(8)],
                vec![peso(1), yuan(4), euro(8)]
            )
            .is_ok()
        );
        assert!(
            value_shuffle_helper(
                vec![peso(1), yuan(4), euro(8)],
                vec![peso(1), euro(8), yuan(4)]
            )
            .is_ok()
        );
        assert!(
            value_shuffle_helper(
                vec![peso(1), yuan(4), euro(8)],
                vec![yuan(4), peso(1), euro(8)]
            )
            .is_ok()
        );
        assert!(
            value_shuffle_helper(
                vec![peso(1), yuan(4), euro(8)],
                vec![yuan(4), euro(8), peso(1)]
            )
            .is_ok()
        );
        assert!(
            value_shuffle_helper(
                vec![peso(1), yuan(4), euro(8)],
                vec![euro(8), peso(1), yuan(4)]
            )
            .is_ok()
        );
        assert!(
            value_shuffle_helper(
                vec![peso(1), yuan(4), euro(8)],
                vec![euro(8), yuan(4), peso(1)]
            )
            .is_ok()
        );
        assert!(
            value_shuffle_helper(
                vec![peso(1), yuan(4), euro(8)],
                vec![wrong(), yuan(4), euro(8)]
            )
            .is_err()
        );
        assert!(
            value_shuffle_helper(
                vec![peso(1), yuan(4), euro(8)],
                vec![peso(1), wrong(), euro(8)]
            )
            .is_err()
        );
        assert!(
            value_shuffle_helper(
                vec![peso(1), yuan(4), euro(8)],
                vec![peso(1), yuan(4), wrong()]
            )
            .is_err()
        );
        assert!(
            value_shuffle_helper(
                vec![Value {
                    q: 0,
                    a: 0u64.into(),
                    t: 0u64.into(),
                }],
                vec![Value {
                    q: 0,
                    a: 0u64.into(),
                    t: 1u64.into(),
                }]
            )
            .is_err()
        );
    }

    fn value_shuffle_helper(input: Vec<Value>, output: Vec<Value>) -> Result<(), SpacesuitError> {
        // Common
        let pc_gens = PedersenGens::default();
        let bp_gens = BulletproofGens::new(128, 1);

        // Prover's scope
        let (proof, input_com, output_com) = {
            let mut prover_transcript = Transcript::new(b"ValueShuffleTest");
            let mut rng = rand::thread_rng();

            let mut prover = Prover::new(&bp_gens, &pc_gens, &mut prover_transcript);
            let (input_com, input_vars) = input.commit(&mut prover, &mut rng);
            let (output_com, output_vars) = output.commit(&mut prover, &mut rng);

            assert!(fill_cs(&mut prover, input_vars, output_vars).is_ok());

            let proof = prover.prove()?;
            (proof, input_com, output_com)
        };

        // Verifier makes a `ConstraintSystem` instance representing a shuffle gadget
        let mut verifier_transcript = Transcript::new(b"ValueShuffleTest");
        let mut verifier = Verifier::new(&bp_gens, &pc_gens, &mut verifier_transcript);

        let input_vars = input_com.commit(&mut verifier);
        let output_vars = output_com.commit(&mut verifier);

        // Verifier adds constraints to the constraint system
        assert!(fill_cs(&mut verifier, input_vars, output_vars).is_ok());

        // Verifier verifies proof
        Ok(verifier.verify(&proof)?)
    }
}
