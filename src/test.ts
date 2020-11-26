import {System} from "./System";
import {registerArithmeticComponents} from "./components/arithmetic";
import {registerBasicComponents} from "./components/basic";
import {Component, ComponentLogic} from "./Component";

let system = new System();

registerBasicComponents(system);
registerArithmeticComponents(system);

/*
let fulladder = system.createComponent("fulladder", "full-adder");

system.setMainComponent(fulladder);
system.constructGraph();

for (let i = 0; i < 8; i++) {
    let A = (i & 1) >> 0;
    let B = (i & 2) >> 1;
    let Cin = (i & 4) >> 2;
    fulladder.getInputPin("A").write(A, 1);
    fulladder.getInputPin("B").write(B, 1);
    fulladder.getInputPin("Cin").write(Cin, 1);
    system.runLogic();
    let S = fulladder.getOutputPin("S").read();
    let Cout = fulladder.getOutputPin("Cout").read();
    console.log(`${A}+${B}+${Cin} = ${Cout}${S}`);
}
*/

function testPinsEqual(pins1: { [key: string]: number }, pins2: { [key: string]: number }): boolean {
    let keys1 = Object.keys(pins1);
    let keys2 = Object.keys(pins2);
    if (keys1.length !== keys2.length) {
        return false;
    }

    for (let key of keys1) {
        if (pins1[key] !== pins2[key]) {
            return false;
        }
    }
    // for (let key of keys2) {
    //     if (pins1[key] !== pins2[key]) return false;
    // }

    return true;
}

function padLeft(text: string, length: number) {
    if (text.length >= length) return text;
    let n = length - text.length;
    let r = "";
    for (let i = 0; i < n; i++) {
        r += " ";
    }
    r += text;
    return r;
}

function printPins(pins: { [key: string]: number }) {
    let keys = Object.keys(pins);
    let maxKeyLength = Math.max(...keys.map(i => i.length));
    for (let key of keys) {
        let value = pins[key];
        console.log(`${padLeft(key, maxKeyLength)}: ${value}`);
    }
}

function print2Pins(pins1: { [key: string]: number }, pins2: { [key: string]: number }) {
    let keys1 = Object.keys(pins1);
    let keys2 = Object.keys(pins2);
    let allKeys = new Set<string>([...keys1, ...keys2]);
    let maxKeyLength = Math.max(...([...allKeys.values()].map(i => i.length)));
    for (let key of allKeys) {
        let value1 = pins1[key];
        let value2 = pins2[key];
        if (value1 === value2) {
            console.log(`${padLeft(key, maxKeyLength)}: ${value1}`);
        } else {
            console.log(`${padLeft(key, maxKeyLength)}: expected ${value1} <> actual ${value2}`);
        }
    }
}

function testComponent(system: System, component: Component, inputEntries: { [key: string]: number }[], logic: ComponentLogic) {
    system.setMainComponent(component);
    system.constructGraph();

    for (let inputs of inputEntries) {
        //TODO system.clear清空pin数据
        component.applyInputValues(inputs);
        system.runLogic();

        let outputs1: { [key: string]: number } = {};
        logic(inputs, outputs1);
        let outputs2 = component.getOutputValues();

        if (!testPinsEqual(outputs1, outputs2)) {
            console.log("-----------------------------");
            console.log("--inputs---------------------");
            printPins(inputs);
            console.log("--outputs--------------------");
            print2Pins(outputs1, outputs2);
            console.log("-----------------------------");
            break;
        }
    }
}

const MAX_ALLINPUT_WIDTH = 16;

function generateAllInputEntries(component: Component): { [key: string]: number }[] {
    let pins = [...component.inputPins.values()];

    let bitRanges: { name: string, mask: number, offset: number }[] = [];
    let total = 0;
    for (let pin of pins) {
        bitRanges.push({name: pin.name, offset: total, mask: (1 << pin.width) - 1});
        total = total + pin.width;
    }
    if (total > MAX_ALLINPUT_WIDTH) {
        console.log(`AllInputEntries max width is 16. Actual width is ${total}`);
        return [];
    }

    let r: { [key: string]: number }[] = [];
    const max = 1 << total;
    for (let i = 0; i < max; i++) {
        let input: { [key: string]: number } = {};
        for (let {name, mask, offset} of bitRanges) {
            input[name] = (i >> offset) & mask;
        }
        r.push(input);
    }
    return r;
}

let adder = system.createComponent("adder", "4bit-adder");

let inputEntries = generateAllInputEntries(adder);
testComponent(system, adder, inputEntries, (inputs, outputs) => {
    let x = inputs.A + inputs.B + inputs.Cin;
    outputs.Cout = (x >= 16) ? 1 : 0;
    outputs.S = x & 0b1111;
});

// let r1 = adder.exportTemplate();
// let r2 = system.componentTemplates.get("4bit-adder");
// console.log(JSON.stringify(r1) === JSON.stringify(r2));