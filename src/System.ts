import {Component, LogicRun, Wire} from "./Component";
import {ComponentInput, ComponentNAND, ComponentOR, ComponentOutput, ComponentXOR} from "./ComponentLib";
import {DependencyGraph} from "./DependencyGraph";

export class System {

    inputComponents: Component[];
    outputComponents: Component[];
    components: Component[];
    wires: Wire[];
    runners: LogicRun[];

    constructor() {
        let c1a = new ComponentInput(0);
        let c1b = new ComponentInput(1);
        let c2 = new ComponentXOR();
        let c3 = new ComponentOutput();

        let w1a = new Wire();
        w1a.setFrom(c1a.output);
        w1a.setTo(c2.input1);
        let w1b = new Wire();
        w1b.setFrom(c1b.output);
        w1b.setTo(c2.input2);
        let w2 = new Wire();
        w2.setFrom(c2.output);
        w2.setTo(c3.input);

        this.inputComponents = [c1a, c1b];
        this.outputComponents = [c3];
        this.components = [c1a, c1b, c2, c3];
        this.wires = [w1a, w1b, w2];

        let g = new DependencyGraph<Component, Wire>();
        this.components.forEach(component => g.addVertex(component));
        this.wires.forEach(wire => g.addEdge(wire.fromPin.component, wire.toPin.component, wire));
        this.runners = g.calcOrder();
        console.log(this.runners);
    }

    run() {
        this.runners.forEach(runner => runner.run());
    }

}