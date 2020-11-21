import {System} from "./System";
import {Component, ComponentGenerator} from "./Component";
import {PinType} from "./ComponentTemplate";

let system = new System();
system.registerLibraryComponent("not", (name: string, componentLibrary: Map<string, ComponentGenerator>) => {
    return new class ComponentNOT extends Component {
        run() {
            this.getOutputPin("out").write(this.getInputPin("in").read() ? 0 : 1, 1);
        }
    }(name, false, {
        type: "not",
        inputPins: [{name: "in", width: 1, type: PinType.BOOL}],
        components: [],
        outputPins: [{name: "out", width: 1, type: PinType.BOOL}],
        wires: [],
    }, componentLibrary);
});

let m = system.createCustomComponent("c_not", {
    type: "level1",
    inputPins: [{name: "in", width: 1, type: PinType.BOOL}],
    components: [{name: "c_name", type: "not"}],
    outputPins: [{name: "out", width: 1, type: PinType.BOOL}],
    wires: [
        {name: "i", width: 1, fromComponent: null, fromPin: "in", toComponent: "c_name", toPin: "in"},
        {name: "o", width: 1, fromComponent: "c_name", fromPin: "out", toComponent: null, toPin: "out"},
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