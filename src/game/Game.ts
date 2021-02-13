import {Editor} from "./Editor";

export class Game {

    readonly editor: Editor;

    constructor() {
        this.editor = new Editor(); //TODO 传入画布div
    }
}