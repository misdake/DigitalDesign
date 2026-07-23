use digital_design_code::{external, CircuitWires, External};
use std::any::Any;
use std::marker::PhantomData;

pub trait CpuComponent: Any {
    type Input: Clone;
    type Output: Clone;

    fn build(input: &Self::Input) -> Self::Output;
}

pub trait CpuComponentEmu<T: CpuComponent>: Any {
    fn init_output(input: &T::Input) -> T::Output;
    fn execute(circuit: &mut CircuitWires, input: &T::Input, output: &T::Output);

    fn build(input: &T::Input) -> T::Output
    where
        Self: Sized,
    {
        let output = Self::init_output(input);
        external(EmulatedComponent::<T, Self> {
            input: input.clone(),
            output: output.clone(),
            emu: PhantomData,
        });
        output
    }
}

pub struct EmulatedComponent<T: CpuComponent, E: CpuComponentEmu<T>> {
    input: T::Input,
    output: T::Output,
    emu: PhantomData<E>,
}

impl<T: CpuComponent, E: CpuComponentEmu<T>> External for EmulatedComponent<T, E> {
    fn execute(&mut self, circuit: &mut CircuitWires) {
        E::execute(circuit, &self.input, &self.output);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl<T: CpuComponent, E: CpuComponentEmu<T>> CpuComponent for EmulatedComponent<T, E> {
    type Input = T::Input;
    type Output = T::Output;

    fn build(input: &Self::Input) -> Self::Output {
        E::build(input)
    }
}
