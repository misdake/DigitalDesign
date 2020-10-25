import {Component, LogicRun, Wire} from "./Component";
import {ComponentInput, ComponentNot, ComponentOutput} from "./ComponentLib";
import {DependencyGraph} from "./DependencyGraph";

export class System {

    inputComponents: Component[];
    outputComponents: Component[];
    components: Component[];
    wires: Wire[];
    runners: LogicRun[];

    constructor() {
        let c1 = new ComponentInput(1);
        let c2 = new ComponentNot();
        let c3 = new ComponentOutput();

        let w1 = new Wire();
        w1.setFrom(c1.output, c1);
        w1.setTo(c2.input, c2);
        let w2 = new Wire();
        w2.setFrom(c2.output, c2);
        w2.setTo(c3.input, c3);

        this.inputComponents = [c1];
        this.outputComponents = [c3];
        this.components = [c1, c2, c3];
        this.wires = [w1, w2];

        let g = new DependencyGraph<Component, Wire>();
        this.components.forEach(component => g.addVertex(component));
        this.wires.forEach(wire => g.addEdge(wire.fromComponent, wire.toComponent, wire));
        this.runners = g.calcOrder();
    }

    run() {
        let c1 = this.inputComponents[0] as ComponentInput;
        c1.data = 1 - c1.data;
        this.runners.forEach(runner => runner.run());
    }

}