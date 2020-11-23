import {System} from "../System";
import {PinTemplate, PinType, WireTemplate} from "../ComponentTemplate";

export function registerBasicComponents(system: System) {

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

}