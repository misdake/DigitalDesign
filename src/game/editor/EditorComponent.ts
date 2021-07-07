import {Game} from "../Game";
import {Editor} from "../Editor";
import {GameComp, GameCompPack, GameCompTemplate} from "../GameComp";
import {Events} from "../../util/Events";
import {CELL_SIZE, GAME_WIDTH, PLAYGROUND_HEIGHT} from "../../util/Constants";

export class EditorComponent {
    private game: Game;
    private editor: Editor;
    constructor(game: Game, editor: Editor) {
        this.game = game;
        this.editor = editor;
    }

    private nextCompId = 1;

    createTemplateComponent(template: GameCompTemplate, x: number, y: number): GameComp {
        let pack = new GameCompPack(template, x, y);
        let comp = new GameComp(this.nextCompId++, this.game.system, pack);
        this.game.templates.push(comp);
        this.game.fire(Events.COMPONENT_ADD, comp);
        return comp;
    }

    createRealComponent(template: GameCompTemplate, x: number, y: number) : GameComp {
        let pack = new GameCompPack(template, x, y);
        let comp = new GameComp(this.nextCompId++, this.game.system, pack);
        this.game.fire(Events.COMPONENT_ADD, comp);
        comp.isTemplate = false;
        this.game.components.push(comp);

        this.game.fire(Events.COMPONENT_ADD, comp);
        return comp;
    }
    createRealComponentFromTemplate(templateComp: GameComp) {
        if (!templateComp.isTemplate) return;
        let index = this.game.templates.indexOf(templateComp);
        if (index >= 0) {
            let comp = templateComp;
            comp.isTemplate = false;
            this.game.templates.splice(index, 1);
            this.createTemplateComponent(comp, comp.x, comp.y);
            this.game.components.push(comp);

            // this.game.fire(Events.COMPONENT_ADD, templateComp); TODO 这个是不是应该分开成template和real的两个add

            this.game._editMain_editor("create component", main => {
                main.components["component_" + comp.id] = comp.component;
            });
        }
    }

    removeRealComponent(gameComp: GameComp): boolean {
        const index = this.game.components.indexOf(gameComp);
        if (index > -1) {
            this.game.editor.wire.removeWiresOfCompoment(gameComp);
            this.game.components.splice(index, 1);
            this.game.fire(Events.COMPONENT_REMOVE, gameComp);

            this.game._editMain_editor("remove component", main => {
                delete main.components["component_" + gameComp.id];
            });
            return true;
        }
        return false;
    }

    private testCollision(gameComp: GameComp, x: number, y: number) {
        let a = gameComp;
        return this.game.components.some(b => {
            if (b === a || a.isTemplate || b.isTemplate) return false;
            let xx = (x + a.w + 1 < b.x) || (x > b.x + b.w + 1);
            let yy = (y + a.h <= b.y) || (y >= b.y + b.h);
            let outside = xx || yy;
            return !outside;
        });
    }

    private testInPlayground(gameComp: GameComp, x: number, y: number) {
        let a = gameComp;
        if (a.isTemplate) return false;
        let xx = (x + a.w + 1 < GAME_WIDTH) && (x > 0);
        let yy = (y + a.h <= PLAYGROUND_HEIGHT) && (y >= 0);
        return xx && yy;
    }
    testInTrash(gameComp: GameComp, x: number, y: number) {
        if (gameComp.isTemplate) return false;
        return y < 0;
    }

    tryMoveComponent(gameComp: GameComp, comp: HTMLDivElement, border: HTMLDivElement, x: number, y: number, force: boolean = false) {
        let tx = x * CELL_SIZE;
        let ty = y * CELL_SIZE;

        let collide = this.testCollision(gameComp, x, y);
        let inside = this.testInPlayground(gameComp, x, y);
        let canMove = !collide && inside;

        let trash = this.testInTrash(gameComp, x, y);

        if (force || canMove) {
            gameComp.x = x;
            gameComp.y = y;
            gameComp.fire(Events.COMPONENT_UPDATE, gameComp, {x, y});
            this.game.fire(Events.COMPONENT_UPDATE, gameComp, {x, y});
            comp.style.transform = `translate(${tx}px, ${ty}px)`;
        }
        if (!force) border.style.display = "block";
        border.style.transform = `translate(${tx - CELL_SIZE}px, ${ty}px)`;
        border.style.borderColor = trash ? "red" : canMove ? "white" : "orange";
    }
}