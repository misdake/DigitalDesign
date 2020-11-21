import {DependencyGraph} from "./DependencyGraph";
import {Component, ComponentGenerator, Pin, Wire} from "./Component";
import {ComponentTemplate} from "./ComponentTemplate";

export class System {

    componentGenerators: Map<string, ComponentGenerator> = new Map<string, ComponentGenerator>();

    registerLibraryComponent(type: string, generator: ComponentGenerator) {
        this.componentGenerators.set(type, generator);
    }

    getComponentTemplate(type: string) {
        return this.componentGenerators.get(type);
    }

    createCustomComponent(name: string, template: ComponentTemplate) {
        return new Component(name, true, template, this.componentGenerators);
    }

    createLibraryComponent(name: string, type: string) {
        return this.componentGenerators.get(type)(name, this.componentGenerators); //TODO 检查存在
    }

    private mainComponent: Component;
    private runners: (Component | Pin | Wire)[];

    setMainComponent(component: Component) {
        this.mainComponent = component;
    }

    constructGraph() {
        let g = new DependencyGraph<Component | Pin, Wire>();

        function add(component: Component) {
            for (let child of component.components.values()) {
                add(child);
            }

            if (component.isCustom) {
                //custom component => 添加pin作为vertex，不添加自己，添加内部所有wire
                for (let inputPin of component.inputPins.values()) g.addVertex(inputPin);
                for (let outputPin of component.outputPins.values()) g.addVertex(outputPin);

                for (let wire of component.wires) {
                    let from = wire.fromComponent.isCustom ? wire.fromPin : wire.fromComponent;
                    let to = wire.toComponent.isCustom ? wire.toPin : wire.toComponent;
                    g.addEdge(from, to, wire);
                }
            } else {
                //builtin component => 添加自己作为vertex，没有内部wire
                g.addVertex(component);
            }
        }

        add(this.mainComponent);

        this.runners = g.calcOrder();

        console.log("order-----------------------");
        for (let runner of this.runners) {
            console.log(runner.constructor.name, runner.name);
        }
    }

    runClock() {
        for (let runner of this.runners) {
            runner.run();
        }
        console.log("run-------------------------");
        for (let input of this.mainComponent.inputPins.values()) {
            console.log(`input: ${input.name} => ${input.read()}`);
        }
        for (let output of this.mainComponent.outputPins.values()) {
            console.log(`output: ${output.name} => ${output.read()}`);
        }
    }

}