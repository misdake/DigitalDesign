export class DependencyGraph<Vertex, Edge> {

    // vertex -> multiple<( from vertex -> edge obj )>
    private inEdges = new Map<Vertex, Map<Vertex, Edge>>(); //will be modified
    private outEdges = new Map<Vertex, Map<Vertex, Edge>>(); //read only

    constructor() {
    }

    addVertex(point: Vertex) {
        this.inEdges.set(point, new Map<Vertex, Edge>());
        this.outEdges.set(point, new Map<Vertex, Edge>());
        this.result = null;
    }

    addEdge(from: Vertex, to: Vertex, obj: Edge) {
        this.inEdges.get(to).set(from, obj);
        this.outEdges.get(from).set(to, obj);
        this.result = null;
    }

    private result: (Vertex | Edge)[] = null;

    calcOrder() {
        if (this.result) return this.result;

        let output: (Vertex | Edge)[] = [];

        let emptyVertices: Vertex[] = [];

        let change = true;
        while (this.inEdges.size > 0 && change) {
            change = false;

            this.inEdges.forEach((value, key) => { //可以再减入度的时候就判断，不用现在遍历map
                if (value.size === 0) {
                    change = true;
                    output.push(key);
                    emptyVertices.push(key);
                }
            });

            emptyVertices.forEach(from => {
                this.inEdges.delete(from);

                let outEdges = this.outEdges.get(from);
                outEdges.forEach((edge, to) => {
                    let inEdges = this.inEdges.get(to);
                    let edgeObj = inEdges.get(from);
                    output.push(edgeObj);
                    inEdges.delete(from);
                });
            });

            emptyVertices = [];
        }

        if (this.inEdges.size > 0) {
            console.log("have cycles");
        }
        this.result = output;
        return output;
    }

}

let g = new DependencyGraph<string, string>();
g.addVertex("1");
g.addVertex("2");
g.addVertex("3");
g.addVertex("4");

g.addEdge("1", "2", "a");
g.addEdge("1", "3", "b");
g.addEdge("2", "4", "c");
g.addEdge("3", "4", "d");
g.addEdge("4", "2", "d");

console.log(g.calcOrder());