import {System} from "../System";
import {PinTemplate, PinType, WireTemplate} from "../ComponentTemplate";

export function registerBasicComponents(system: System) {

    for(let width = 1; width <= 4; width++) {
        system.registerBuiltinComponent({
            type: `pass${width}`,
            inputPins: [{name: "in", width: width, type: PinType.BOOL}],
            components: [],
            outputPins: [{name: "out", width: width, type: PinType.BOOL}],
            wires: [],
        }, (inputs, outputs) => {
            outputs.out = inputs.in;
        });
    }

    system.registerBuiltinComponent({
        type: "not",
        inputPins: [{name: "in", width: 1, type: PinType.BOOL}],
        components: [],
        outputPins: [{name: "out", width: 1, type: PinType.BOOL}],
        wires: [],
    }, (inputs, outputs) => {
        outputs.out = inputs.in ? 0 : 1;
    });

    let in2out1 = {
        inputPins: [{name: "in0", width: 1, type: PinType.BOOL}, {name: "in1", width: 1, type: PinType.BOOL}] as PinTemplate[],
        components: [] as ({ name: string, type: string })[],
        outputPins: [{name: "out", width: 1, type: PinType.BOOL}] as PinTemplate[],
        wires: [] as WireTemplate[],
    };
    system.registerBuiltinComponent(Object.assign({type: "and"}, in2out1), (inputs, outputs) => outputs.out = (inputs.in0 && inputs.in1) ? 1 : 0);
    system.registerBuiltinComponent(Object.assign({type: "or"}, in2out1), (inputs, outputs) => outputs.out = (inputs.in0 || inputs.in1) ? 1 : 0);
    system.registerBuiltinComponent(Object.assign({type: "nand"}, in2out1), (inputs, outputs) => outputs.out = (inputs.in0 && inputs.in1) ? 0 : 1);
    system.registerBuiltinComponent(Object.assign({type: "xor"}, in2out1), (inputs, outputs) => outputs.out = (inputs.in0 === inputs.in1) ? 0 : 1);

    //TODO 支持template+logic生成器，用于根据参数生成component

    system.registerBuiltinComponent({
        type: "mux2way1bit",
        inputPins: [{name: "select", width: 1, type: PinType.BOOL}, {name: "in0", width: 1, type: PinType.BOOL}, {name: "in1", width: 1, type: PinType.BOOL}],
        components: [],
        outputPins: [{name: "out", width: 1, type: PinType.BOOL}],
        wires: [],
    }, (inputs, outputs) => {
        outputs.out = inputs.select ? inputs.in1 : inputs.in0;
    });
    system.registerBuiltinComponent({
        type: "dmux2way1bit",
        inputPins: [{name: "in", width: 1, type: PinType.BOOL}],
        components: [],
        outputPins: [{name: "out0", width: 1, type: PinType.BOOL}, {name: "out1", width: 1, type: PinType.BOOL}],
        wires: [],
    }, (inputs, outputs) => {
        outputs.out1 = inputs.select ? 1 : 0;
        outputs.out0 = inputs.select ? 0 : 1;
    });

    system.registerBuiltinComponent({
        type: "pack2",
        inputPins: [{name: "in0", width: 1, type: PinType.BOOL}, {name: "in1", width: 1, type: PinType.BOOL}],
        components: [],
        outputPins: [{name: "out", width: 2, type: PinType.BOOL}],
        wires: [],
    }, (inputs, outputs) => {
        outputs.out = (inputs.in1 << 1) + inputs.in0;
    });
    system.registerBuiltinComponent({
        type: "unpack2",
        inputPins: [{name: "in", width: 2, type: PinType.BOOL}],
        components: [],
        outputPins: [{name: "out0", width: 1, type: PinType.BOOL}, {name: "out1", width: 1, type: PinType.BOOL}],
        wires: [],
    }, (inputs, outputs) => {
        outputs.out0 = (inputs.in & 1) << 0;
        outputs.out1 = (inputs.in & 2) << 1;
    });
    system.registerBuiltinComponent({
        type: "pack4",
        inputPins: [{name: "in0", width: 1, type: PinType.BOOL}, {name: "in1", width: 1, type: PinType.BOOL}, {name: "in2", width: 1, type: PinType.BOOL}, {name: "in3", width: 1, type: PinType.BOOL}],
        components: [],
        outputPins: [{name: "out", width: 4, type: PinType.BOOL}],
        wires: [],
    }, (inputs, outputs) => {
        outputs.out = (inputs.in3 << 3) + (inputs.in2 << 2) + (inputs.in1 << 1) + (inputs.in0 << 0);
    });
    system.registerBuiltinComponent({
        type: "unpack4",
        inputPins: [{name: "in", width: 4, type: PinType.BOOL}],
        components: [],
        outputPins: [{name: "out0", width: 1, type: PinType.BOOL}, {name: "out1", width: 1, type: PinType.BOOL}, {name: "out2", width: 1, type: PinType.BOOL}, {name: "out3", width: 1, type: PinType.BOOL}],
        wires: [],
    }, (inputs, outputs) => {
        outputs.out0 = (inputs.in >> 0) & 1;
        outputs.out1 = (inputs.in >> 1) & 1;
        outputs.out2 = (inputs.in >> 2) & 1;
        outputs.out3 = (inputs.in >> 3) & 1;
    });

}