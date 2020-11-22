import {System} from "./System";
import {Component, ComponentGenerator} from "./Component";
import {PinType} from "./ComponentTemplate";

let system = new System();
// system.registerLibraryComponent("not", (name: string, componentLibrary: Map<string, ComponentGenerator>) => {
//     return new class ComponentNOT extends Component {
//         run() {
//             this.getOutputPin("out").write(this.getInputPin("in").read() ? 0 : 1, 1);
//         }
//     }(name, false, {
//         type: "not",
//         inputPins: [{name: "in", width: 1, type: PinType.BOOL}],
//         components: [],
//         outputPins: [{name: "out", width: 1, type: PinType.BOOL}],
//         wires: [],
//     }, componentLibrary);
// });
system.registerLibraryComponent("nand", (name: string, componentLibrary: Map<string, ComponentGenerator>) => {
    return new class ComponentNOT extends Component {
        run() {
            let in1 = this.getInputPin("in1_nand").read();
            let in2 = this.getInputPin("in2_nand").read();
            this.getOutputPin("out_nand").write((in1 && in2) ? 0 : 1, 1);
        }
    }(name, false, {
        type: "not",
        inputPins: [{name: "in1_nand", width: 1, type: PinType.BOOL}, {name: "in2_nand", width: 1, type: PinType.BOOL}],
        components: [],
        outputPins: [{name: "out_nand", width: 1, type: PinType.BOOL}],
        wires: [],
    }, componentLibrary);
});

let m = system.createCustomComponent("c_not", {
    type: "level1",
    inputPins: [{name: "in", width: 1, type: PinType.BOOL}],
    components: [{name: "c_name", type: "nand"}],
    outputPins: [{name: "out_not", width: 1, type: PinType.BOOL}],
    wires: [
        {name: "wi1_not", width: 1, fromComponent: null, fromPin: "in", toComponent: "c_name", toPin: "in1_nand"},
        {name: "wi2_not", width: 1, fromComponent: null, fromPin: "in", toComponent: "c_name", toPin: "in2_nand"},
        {name: "wout_not", width: 1, fromComponent: "c_name", fromPin: "out_nand", toComponent: null, toPin: "out_not"},
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