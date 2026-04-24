uniform sampler2D s0;

struct PS_INPUT {
    float4 t0 : TEXCOORD0;
};

struct PS_OUTPUT {
    float4 oC0 : COLOR0;
};

PS_OUTPUT main(PS_INPUT input) {
    PS_OUTPUT output;
    output.oC0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r1 = float4(0.0, 0.0, 0.0, 0.0);

    r0 = tex2D(s0, input.t0.xy);
    r1.w = dot(c0, r0);
    r1.x = (-(r1.w) + c1.w);
    r1.y = (r0.w * c2.x);
    r0.w = (r1.x >= 0 ? r1.y : r0.w);
    output.oC0 = r0;
    return output;
}
