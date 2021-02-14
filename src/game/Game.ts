import {Editor} from "./Editor";
import {GameComp} from "./GameComp";
import {System} from "../logic/System";
import {registerBasicComponents} from "../logic/components/basic";

export class Game {
    readonly system: System;

    readonly components: GameComp[];
    readonly editor: Editor;

    constructor() {
        this.system = new System();
        registerBasicComponents(this.system);

        this.components = [];

        this.editor = new Editor(this); //TODO 传入画布div
    }

}