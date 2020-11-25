import {System} from "./System";
import {registerArithmeticComponents} from "./components/arithmetic";
import {registerBasicComponents} from "./components/basic";

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

let adder = system.createComponent("adder", "4bit-adder");

system.setMainComponent(adder);
system.constructGraph();

for (let i = 0; i < 15; i++) {
    for (let j = 0; j < 15; j++) {
        let A = i;
        let B = j;
        let Cin = 0;
        adder.getInputPin("A").write(A, 4);
        adder.getInputPin("B").write(B, 4);
        adder.getInputPin("Cin").write(Cin, 1);
        system.runLogic();
        let S = adder.getOutputPin("S").read();
        let Cout = adder.getOutputPin("Cout").read();
        console.log(`${A}+${B}+${Cin} = ${Cout ? 'Cout ' : ''}${S}`);
    }
}

let r1 = adder.exportTemplate();
let r2 = system.componentTemplates.get("4bit-adder");
console.log(JSON.stringify(r1) === JSON.stringify(r2));