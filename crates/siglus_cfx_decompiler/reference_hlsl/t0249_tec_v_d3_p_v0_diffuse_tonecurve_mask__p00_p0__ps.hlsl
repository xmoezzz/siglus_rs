uniform sampler2D s0;
uniform sampler2D s1;

struct PS_INPUT {
    float4 v0 : COLOR0;
    float4 t1 : TEXCOORD1;
};

struct PS_OUTPUT {
    float4 oC0 : COLOR0;
};

PS_OUTPUT main(PS_INPUT input) {
    PS_OUTPUT output;
    output.oC0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r1 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r2 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r3 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r4 = float4(0.0, 0.0, 0.0, 0.0);

    r0 = tex2D(s0, input.t1.xy);
    r1.w = input.v0.w;
    r2.w = dot(c0, input.v0);
    r3.xyz = lerp(input.v0.xyz, r2.www, c1.yyy);
    r3.w = c1.x;
    r2.x = r3.x;
    r2.y = r3.w;
    r3.x = r3.y;
    r3.y = r3.w;
    r4.x = r3.z;
    r4.y = r3.w;
    r2 = tex2D(s1, r2.xy);
    r3 = tex2D(s1, r3.xy);
    r4 = tex2D(s1, r4.xy);
    r1.x = r2.x;
    r1.y = r3.y;
    r1.z = r4.z;
    r0 = (r0 * r1);
    output.oC0 = r0;
    return output;
}
