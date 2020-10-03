import {arrayOf, InputPin, OutputPin} from "./Pin";

export class Component {
    name: string;
    inputPins: InputPin[];
    outputPins: OutputPin[];
    inputs: { [key: string]: InputPin };
    outputs: { [key: string]: OutputPin };

    constructor(name: string, inputPins: { name: string, width: number }[], outputPins: { name: string, width: number }[]) {
        this.name = name;

        this.inputPins = [];
        this.outputPins = [];
        this.inputs = {};
        this.outputs = {};

        inputPins.forEach(inputPin => {
            let i = new InputPin(inputPin.name, this, inputPin.width);
            this.inputPins.push(i);
            this.inputs[inputPin.name] = i;
        });
        outputPins.forEach(outputPin => {
            let i = new OutputPin(outputPin.name, this, outputPin.width);
            this.outputPins.push(i);
            this.outputs[outputPin.name] = i;
        });
    }
}

export class Reg extends Component {
    constructor(name: string, width: number) {
        super(
            name,
            [{name: "nextValue", width: width}, {name: "writeEnable", width: 1}],
            [{name: "currValue", width: width}],
        );
        this.width = width;
        this.nextValue = this.inputs["nextValue"];
        this.writeEnable = this.inputs["writeEnable"];
        this.currValue = this.outputs["currValue"];
    }

    nextValue: InputPin;
    writeEnable: InputPin;

    width: number;
    value: number[];

    currValue: OutputPin;

    initialize() {
        this.value = arrayOf(0, this.width);
    }

    startCycle() {
        this.currValue.write(this.value);
    }

    endCycle() {
        if (this.writeEnable) {
            this.value = this.nextValue.readAll();
        }
    }
}

export class Logic extends Component {
    execute() {

    }
}

export class Not extends Logic {
    constructor() {
        super(
            "Not",
            [{name: "input", width: 1}],
            [{name: "output", width: 1}],
        );
    }

    execute() {
        this.outputs["output"].write([this.inputs["input"].read(0) ? 0 : 1]);
    }
}

