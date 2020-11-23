import {System} from "../System";
import {PinTemplate, PinType, WireTemplate} from "../ComponentTemplate";
import {ComponentGenerator} from "../Component";

export function registerArithmeticComponents(system: System) {
    system.registerLibraryComponent("full-adder", (name: string, componentLibrary: Map<string, ComponentGenerator>) => {
        return system.createCustomComponent(name, {
            type: "full-adder",
            inputPins: [

                {name: "A", width: 1, type: PinType.BOOL},
                {name: "B", width: 1, type: PinType.BOOL},
                {name: "Cin", width: 1, type: PinType.BOOL},

            ] as PinTemplate[],
            components: [

                {name: "xor1", type: "xor"},
                {name: "xor2", type: "xor"},
                {name: "and1", type: "and"},
                {name: "and2", type: "and"},
                {name: "or", type: "or"},

            ] as ({ name: string, type: string })[],
            outputPins: [

                {name: "S", width: 1, type: PinType.BOOL},
                {name: "Cout", width: 1, type: PinType.BOOL},

            ] as PinTemplate[],

            wires: [

                //低位
                WireTemplate.create("this.A", "xor1.in1"),
                WireTemplate.create("this.B", "xor1.in2"),
                WireTemplate.create("xor1.out", "xor2.in1"),
                WireTemplate.create("this.Cin", "xor2.in2"),
                WireTemplate.create("xor2.out", "this.S"),

                //进位
                WireTemplate.create("this.A", "and1.in1"),
                WireTemplate.create("this.B", "and1.in2"),
                WireTemplate.create("xor1.out", "and2.in1"),
                WireTemplate.create("this.Cin", "and2.in2"),
                WireTemplate.create("and1.out", "or.in1"),
                WireTemplate.create("and2.out", "or.in2"),
                WireTemplate.create("or.out", "this.Cout"),

            ] as WireTemplate[],
        });
    });
}