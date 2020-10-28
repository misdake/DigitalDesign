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
        w1a.setFrom(c1a.output, c1a);
        w1a.setTo(c2.input1, c2);
        let w1b = new Wire();
        w1b.setFrom(c1b.output, c1b);
        w1b.setTo(c2.input2, c2);
        let w2 = new Wire();
        w2.setFrom(c2.output, c2);
        w2.setTo(c3.input, c3);

        this.inputComponents = [c1a, c1b];
        this.outputComponents = [c3];
        this.components = [c1a, c1b, c2, c3];
        this.wires = [w1a, w1b, w2];

        let g = new DependencyGraph<Component, Wire>();
        this.components.forEach(component => g.addVertex(component));
        this.wires.forEach(wire => g.addEdge(wire.fromComponent, wire.toComponent, wire));
        this.runners = g.calcOrder();
        console.log(this.runners);
    }

    run() {
        this.runners.forEach(runner => runner.run());
    }

}