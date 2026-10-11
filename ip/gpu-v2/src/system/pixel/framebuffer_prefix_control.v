                // Private lane writes are never externally flushed or evicted
                // before the ordered owner retires. A fault abandons the tile.
                if(!source_bad)begin
                    if(depth_pending)begin
                        depth_pending<=0;
                        if(depth_key==2'd3)begin
                            if(quad_color_dirty)dirc[rop_line]<=1;
                            if(quad_depth_dirty)dird[rop_line]<=1;
                            quad_color_dirty<=0;quad_depth_dirty<=0;
                            commit_valid<=1;commit_x<=s_hx[head];
                            commit_y<=s_hy[head];commit_mask<=s_hm[head];
                            slot_ready[head]<=0;head<=~head;
                        end
                    end
                    if(return_fire)begin
                        depth_pending<=1;depth_data<=leaf_new_depth;
                        depth_key<=leaf_out_key;depth_flag<=leaf_dw;
                        quad_color_dirty<=quad_color_dirty||leaf_cw;
                        quad_depth_dirty<=quad_depth_dirty||leaf_dw;
                        return_key<=return_key+2'd1;
                    end
                end
