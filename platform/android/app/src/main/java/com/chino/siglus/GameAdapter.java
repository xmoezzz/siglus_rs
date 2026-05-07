package com.chino.siglus;

import android.net.Uri;
import android.view.LayoutInflater;
import android.view.View;
import android.view.ViewGroup;
import android.widget.ImageView;
import android.widget.TextView;

import androidx.annotation.NonNull;
import androidx.recyclerview.widget.RecyclerView;

import java.io.File;
import java.util.ArrayList;
import java.util.List;

public final class GameAdapter extends RecyclerView.Adapter<GameAdapter.Holder> {

    public interface Listener {
        void onGameClicked(GameEntry e);
        void onGameLongPressed(GameEntry e);
    }

    private final Listener listener;
    private final List<GameEntry> items = new ArrayList<>();

    public GameAdapter(Listener listener) {
        this.listener = listener;
    }

    public void setItems(List<GameEntry> newItems) {
        items.clear();
        if (newItems != null) {
            items.addAll(newItems);
        }
        notifyDataSetChanged();
    }

    @NonNull
    @Override
    public Holder onCreateViewHolder(@NonNull ViewGroup parent, int viewType) {
        View v = LayoutInflater.from(parent.getContext()).inflate(R.layout.item_game, parent, false);
        return new Holder(v);
    }

    @Override
    public void onBindViewHolder(@NonNull Holder holder, int position) {
        GameEntry e = items.get(position);
        holder.title.setText(e.title);
        holder.nls.setText(e.nls);
        if (e.coverPath != null && !e.coverPath.isEmpty() && new File(e.coverPath).isFile()) {
            holder.cover.setImageURI(Uri.fromFile(new File(e.coverPath)));
            holder.cover.setVisibility(View.VISIBLE);
            holder.title.setVisibility(View.GONE);
        } else {
            holder.cover.setImageDrawable(null);
            holder.cover.setVisibility(View.GONE);
            holder.title.setVisibility(View.VISIBLE);
        }
        holder.itemView.setOnClickListener(v -> listener.onGameClicked(e));
        holder.itemView.setOnLongClickListener(v -> {
            listener.onGameLongPressed(e);
            return true;
        });
    }

    @Override
    public int getItemCount() {
        return items.size();
    }

    static final class Holder extends RecyclerView.ViewHolder {
        final ImageView cover;
        final TextView title;
        final TextView nls;
        Holder(@NonNull View itemView) {
            super(itemView);
            cover = itemView.findViewById(R.id.img_cover);
            title = itemView.findViewById(R.id.txt_title);
            nls = itemView.findViewById(R.id.txt_nls);
        }
    }
}
