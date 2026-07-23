use crate::{external, CircuitWires, External};
use std::any::Any;

pub trait CircuitComponent: Any {
    type Input: Clone;
    type Output: Clone;

    fn build(input: &Self::Input) -> Self::Output;
}

pub trait CircuitComponentEmu<T: CircuitComponent>: Any + Sized {
    fn create(input: &T::Input) -> (Self, T::Output);
    fn execute(&mut self, circuit: &mut CircuitWires, input: &T::Input, output: &T::Output);

    fn build(input: &T::Input) -> T::Output {
        let (emu, output) = Self::create(input);
        external(EmulatedComponent::<T, Self> {
            input: input.clone(),
            output: output.clone(),
            emu,
        });
        output
    }
}

pub struct EmulatedComponent<T: CircuitComponent, E: CircuitComponentEmu<T>> {
    input: T::Input,
    output: T::Output,
    emu: E,
}

impl<T: CircuitComponent, E: CircuitComponentEmu<T>> External for EmulatedComponent<T, E> {
    fn execute(&mut self, circuit: &mut CircuitWires) {
        self.emu.execute(circuit, &self.input, &self.output);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl<T: CircuitComponent, E: CircuitComponentEmu<T>> CircuitComponent for EmulatedComponent<T, E> {
    type Input = T::Input;
    type Output = T::Output;

    fn build(input: &Self::Input) -> Self::Output {
        E::build(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_circuit, input, Wire};

    struct TestComponent;

    impl CircuitComponent for TestComponent {
        type Input = Wire;
        type Output = Wire;

        fn build(_input: &Self::Input) -> Self::Output {
            unreachable!()
        }
    }

    struct StatefulEmu {
        invert: bool,
    }

    impl CircuitComponentEmu<TestComponent> for StatefulEmu {
        fn create(_input: &Wire) -> (Self, Wire) {
            (Self { invert: false }, input())
        }

        fn execute(&mut self, circuit: &mut CircuitWires, input: &Wire, output: &Wire) {
            let value = input.get(circuit) ^ u8::from(self.invert);
            output.set(circuit, value);
            self.invert = !self.invert;
        }
    }

    #[test]
    fn emulated_component_keeps_state_between_executions() {
        type EmulatedTestComponent = EmulatedComponent<TestComponent, StatefulEmu>;

        let (mut circuit, (input, output)) = build_circuit(|| {
            let input = input();
            let output = EmulatedTestComponent::build(&input);
            (input, output)
        });

        input.set(&mut circuit, 1);
        circuit.execute_gates();
        assert_eq!(output.get(&circuit), 1);

        circuit.execute_gates();
        assert_eq!(output.get(&circuit), 0);
    }
}
