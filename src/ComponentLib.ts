import {component, Component, inputPin, InputPin, outputPin, OutputPin} from "./Component";

@component
export class ComponentNot extends Component {
    @inputPin(1)
    input: InputPin;

    @outputPin(1)
    output: OutputPin;

    run() {
        this.output.write(this.input.read() ? 0 : 1);
    }
}

export class ComponentInput extends Component {
    constructor(data: number) {
        super();
        this.data = data;
    }

    data: number;

    @outputPin(1)
    output: OutputPin;

    run() {
        this.output.write(this.data);
    }
}

export class ComponentOutput extends Component {
    @inputPin(1)
    input: InputPin;

    run() {
        console.log("output:", this.input.read());
    }
}