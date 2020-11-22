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

                //TODO Wire不保存宽度，自动判断
                //TODO component+pin用更简单的方式识别，比如"xor1.out"

                //低位
                {name: "", width: 1, fromComponent: null, fromPin: "A", toComponent: "xor1", toPin: "in1"},
                {name: "", width: 1, fromComponent: null, fromPin: "B", toComponent: "xor1", toPin: "in2"},
                {name: "", width: 1, fromComponent: "xor1", fromPin: "out", toComponent: "xor2", toPin: "in1"},
                {name: "", width: 1, fromComponent: null, fromPin: "Cin", toComponent: "xor2", toPin: "in2"},
                {name: "", width: 1, fromComponent: "xor2", fromPin: "out", toComponent: null, toPin: "S"},

                //进位
                {name: "", width: 1, fromComponent: null, fromPin: "A", toComponent: "and1", toPin: "in1"},
                {name: "", width: 1, fromComponent: null, fromPin: "B", toComponent: "and1", toPin: "in2"},
                {name: "", width: 1, fromComponent: "xor1", fromPin: "out", toComponent: "and2", toPin: "in1"},
                {name: "", width: 1, fromComponent: null, fromPin: "Cin", toComponent: "and2", toPin: "in2"},
                {name: "", width: 1, fromComponent: "and1", fromPin: "out", toComponent: "or", toPin: "in1"},
                {name: "", width: 1, fromComponent: "and2", fromPin: "out", toComponent: "or", toPin: "in2"},
                {name: "", width: 1, fromComponent: "or", fromPin: "out", toComponent: null, toPin: "Cout"},

            ] as WireTemplate[],
        });
    });
}