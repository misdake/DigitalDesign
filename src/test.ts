import {System} from "./System";
import {registerArithmeticComponents} from "./components/arithmetic";
import {registerBasicComponents} from "./components/basic";

let system = new System();

registerBasicComponents(system);
registerArithmeticComponents(system);

let fulladder = system.createLibraryComponent("fulladder", "full-adder");

system.setMainComponent(fulladder);
system.constructGraph();

fulladder.getInputPin("A").write(0, 1);
fulladder.getInputPin("B").write(0, 1);
fulladder.getInputPin("Cin").write(0, 1);
system.runClock();

fulladder.getInputPin("A").write(1, 1);
fulladder.getInputPin("B").write(0, 1);
fulladder.getInputPin("Cin").write(0, 1);
system.runClock();

fulladder.getInputPin("A").write(0, 1);
fulladder.getInputPin("B").write(1, 1);
fulladder.getInputPin("Cin").write(0, 1);
system.runClock();

fulladder.getInputPin("A").write(1, 1);
fulladder.getInputPin("B").write(1, 1);
fulladder.getInputPin("Cin").write(0, 1);
system.runClock();

fulladder.getInputPin("A").write(0, 1);
fulladder.getInputPin("B").write(0, 1);
fulladder.getInputPin("Cin").write(1, 1);
system.runClock();

fulladder.getInputPin("A").write(1, 1);
fulladder.getInputPin("B").write(0, 1);
fulladder.getInputPin("Cin").write(1, 1);
system.runClock();

fulladder.getInputPin("A").write(0, 1);
fulladder.getInputPin("B").write(1, 1);
fulladder.getInputPin("Cin").write(1, 1);
system.runClock();

fulladder.getInputPin("A").write(1, 1);
fulladder.getInputPin("B").write(1, 1);
fulladder.getInputPin("Cin").write(1, 1);
system.runClock();