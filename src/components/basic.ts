import {System} from "../System";
import {Component, ComponentGenerator} from "../Component";
import {PinTemplate, PinType, WireTemplate} from "../ComponentTemplate";

export function registerBasicComponents(system: System) {

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

    let in2out1 = {
        inputPins: [{name: "in1", width: 1, type: PinType.BOOL}, {name: "in2", width: 1, type: PinType.BOOL}] as PinTemplate[],
        components: [] as ({ name: string, type: string })[],
        outputPins: [{name: "out", width: 1, type: PinType.BOOL}] as PinTemplate[],
        wires: [] as WireTemplate[],
    };
    system.registerLibraryComponent("and", (name: string, componentLibrary: Map<string, ComponentGenerator>) => {
        return new class ComponentNOT extends Component {
            run() {
                let in1 = this.getInputPin("in1").read();
                let in2 = this.getInputPin("in2").read();
                this.getOutputPin("out").write((in1 && in2) ? 1 : 0, 1);
            }
        }(name, false, Object.assign({type: "and"}, in2out1), componentLibrary);
    });
    system.registerLibraryComponent("or", (name: string, componentLibrary: Map<string, ComponentGenerator>) => {
        return new class ComponentNOT extends Component {
            run() {
                let in1 = this.getInputPin("in1").read();
                let in2 = this.getInputPin("in2").read();
                this.getOutputPin("out").write((in1 || in2) ? 1 : 0, 1);
            }
        }(name, false, Object.assign({type: "and"}, in2out1), componentLibrary);
    });
    system.registerLibraryComponent("nand", (name: string, componentLibrary: Map<string, ComponentGenerator>) => {
        return new class ComponentNOT extends Component {
            run() {
                let in1 = this.getInputPin("in1").read();
                let in2 = this.getInputPin("in2").read();
                this.getOutputPin("out").write((in1 && in2) ? 0 : 1, 1);
            }
        }(name, false, Object.assign({type: "nand"}, in2out1), componentLibrary);
    });
    system.registerLibraryComponent("xor", (name: string, componentLibrary: Map<string, ComponentGenerator>) => {
        return new class ComponentNOT extends Component {
            run() {
                let in1 = this.getInputPin("in1").read();
                let in2 = this.getInputPin("in2").read();
                this.getOutputPin("out").write((in1 !== in2) ? 1 : 0, 1);
            }
        }(name, false, Object.assign({type: "xor"}, in2out1), componentLibrary);
    });


}