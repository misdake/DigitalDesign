import {System} from "./System";
import {registerArithmeticComponents} from "./components/arithmetic";
import {registerBasicComponents} from "./components/basic";

let system = new System();

registerBasicComponents(system);
registerArithmeticComponents(system);

let fulladder = system.createLibraryComponent("fulladder", "full-adder");

system.setMainComponent(fulladder);
system.constructGraph();

for (let i = 0; i < 8; i++) {
    let A = (i & 1) >> 0;
    let B = (i & 2) >> 1;
    let Cin = (i & 4) >> 2;
    fulladder.getInputPin("A").write(A, 1);
    fulladder.getInputPin("B").write(B, 1);
    fulladder.getInputPin("Cin").write(Cin, 1);
    system.runClock();
    let S = fulladder.getOutputPin("S").read();
    let Cout = fulladder.getOutputPin("Cout").read();
    console.log(`${A}+${B}+${Cin} = ${Cout}${S}`);
}