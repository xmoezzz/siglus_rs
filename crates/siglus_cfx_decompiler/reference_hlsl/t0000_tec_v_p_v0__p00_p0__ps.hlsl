
struct PS_INPUT {
    float4 v0 : COLOR0;
};

struct PS_OUTPUT {
    float4 oC0 : COLOR0;
};

PS_OUTPUT main(PS_INPUT input) {
    PS_OUTPUT output;
    output.oC0 = float4(0.0, 0.0, 0.0, 0.0);

    output.oC0 = input.v0;
    return output;
}
