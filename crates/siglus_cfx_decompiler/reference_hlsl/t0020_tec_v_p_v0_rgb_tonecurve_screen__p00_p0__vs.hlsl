
struct VS_INPUT {
    float4 v0 : POSITION0;
    float4 v1 : TEXCOORD0;
    float4 v2 : COLOR0;
};

struct VS_OUTPUT {
    float4 oPos : POSITION0;
    float4 oD0 : COLOR0;
    float4 o0 : TEXCOORD0;
};

VS_OUTPUT main(VS_INPUT input) {
    VS_OUTPUT output;
    output.oPos = float4(0.0, 0.0, 0.0, 0.0);
    output.oD0 = float4(0.0, 0.0, 0.0, 0.0);
    output.o0 = float4(0.0, 0.0, 0.0, 0.0);

    output.oPos = input.v0;
    output.o0.xy = input.v1.xy;
    output.oD0 = input.v2;
    return output;
}
