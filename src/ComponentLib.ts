import {component, Component, inputPin, InputPin, outputPin, OutputPin} from "./Component";

@component
export class ComponentInput extends Component {
    data: number;

    @outputPin(1)
    output: OutputPin;

    constructor(data: number) {
        super();
        this.data = data;
    }

    run() {
        console.log("input:", this.data);
        this.output.write(this.data);
    }
}

@component
export class ComponentOutput extends Component {
    @inputPin(1)
    input: InputPin;

    run() {
        console.log("output:", this.input.read());
    }
}

@component
export class ComponentNOT extends Component {
    @inputPin(1)
    input: InputPin;
    @outputPin(1)
    output: OutputPin;

    run() {
        this.output.write(this.input.read() ? 0 : 1);
    }
}

@component
export class ComponentNAND extends Component {
    @inputPin(1)
    input1: InputPin;
    @inputPin(1)
    input2: InputPin;
    @outputPin(1)
    output: OutputPin;

    run() {
        let read1 = this.input1.read1();
        let read2 = this.input2.read1();
        let r = (read1 & read2) ? 0 : 1;
        console.log("nand", read1, read2, r);
        this.output.write(r);
    }
}