import {System} from "./System";
import {PinType} from "./ComponentTemplate";
import {registerBasicComponents} from "./components/basic";

let system = new System();

registerBasicComponents(system);

let m = system.createCustomComponent("c_not", {
    type: "level1",
    inputPins: [{name: "in", width: 1, type: PinType.BOOL}],
    components: [{name: "nand", type: "nand"}],
    outputPins: [{name: "out_not", width: 1, type: PinType.BOOL}],
    wires: [
        {name: "wireIn1", width: 1, fromComponent: null, fromPin: "in", toComponent: "nand", toPin: "in1"},
        {name: "wireIn2", width: 1, fromComponent: null, fromPin: "in", toComponent: "nand", toPin: "in2"},
        {name: "wireOut", width: 1, fromComponent: "nand", fromPin: "out", toComponent: null, toPin: "out_not"},
    ],
});
system.setMainComponent(m);
system.constructGraph();

m.getInputPin("in").write(1, 1);
system.runClock();
m.getInputPin("in").write(0, 1);
system.runClock();
m.getInputPin("in").write(1, 1);
system.runClock();